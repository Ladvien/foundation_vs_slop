//! Dimensional-crab swarm — a wall-climbing infestation enemy.
//!
//! Where the smiley boss (`crate::enemy`) is one slow bullet-sponge, the crabs are a ~40-strong swarm:
//! individually weak (a shot or two kills one) but lethal when several pile onto a unit. Their defining
//! trait is that they navigate the dungeon's *walls* as freely as its floors — mounting a wall from the
//! floor, crawling up the face and along the run, rounding corners, and dropping back down — chasing the
//! squad across the 2.5D surface manifold built by `crate::surface_nav`.
//!
//! Locomotion mirrors `enemy::enemy_seek`'s snapshot-then-move shape, but on surfaces: one `Arc`-shared
//! surface flow field (rebuilt only when the squad crosses cell boundaries) tells every crab which
//! neighbouring patch descends toward the nearest unit; the crab steers along its current surface's
//! tangent plane toward that patch's gate, transfers patch when it reaches the boundary, and re-orients
//! flat to the new surface (the `Quat::from_rotation_arc` trick `gore` uses for wall decals). Reynolds
//! separation (Reynolds, "Steering Behaviors For Autonomous Characters", GDC 1999) keeps the swarm from
//! collapsing to a point; it is evaluated through a per-frame spatial hash so 40 agents stay O(n·k), not
//! O(n²) (the smiley's pairwise loop is fine for one enemy, not a swarm).
//!
//! Entity shape (root scale 1 so the raycast collider keeps world size; the model is a scaled child):
//!   root  = `Crab` + `Hostile` + `Health` + `CrabMotion`/`CrabState` + a material-less `Sphere` `Mesh3d`
//!           (the invisible laser hit target, exactly like the smiley capsule).
//!   child = `WorldAssetRoot` of `dimensional_crab.glb#Scene0`, scaled to ~0.5 m and seated on the body.

pub(crate) use std::collections::HashMap;
pub(crate) use std::sync::Arc;
pub(crate) use std::time::Duration;

pub(crate) use avian3d::prelude::{AngularVelocity, LinearVelocity, RigidBody};
pub(crate) use bevy::prelude::*;

pub(crate) use crate::audio::Sfx;
pub(crate) use crate::dungeon::Dungeon;
pub(crate) use crate::enemy::Hostile;
pub(crate) use crate::gore::{GoreEvent, GoreKind, GoreQueue};
pub(crate) use crate::health::{Biological, Health, NoHealthBar};
pub(crate) use crate::behavior_tuning::{BehaviorTuning, CrabTuning};
pub(crate) use crate::sim::SimTuning;
pub(crate) use crate::squad::{Prey, Unit};
pub(crate) use crate::surface_nav::{clamp_to_patch, project_tangent, surface_orientation, SurfaceField, SurfaceGraph};
pub(crate) use crate::util::{hash01_u32, rand01, unit_is_facing};

mod setup;
mod movement;
mod combat;
mod foraging;
pub(crate) use setup::*;
pub(crate) use movement::*;
pub(crate) use combat::*;
pub(crate) use foraging::*;

/// Total crabs across the level, split into `CRAB_CLUSTERS` nests in far rooms.
pub(crate) const CRAB_COUNT: usize = 40;
pub(crate) const CRAB_CLUSTERS: usize = 4;
/// The first this-many clusters are seeded directly on wall faces, so wall-climbing is always visible
/// (the rest start on the floor and mount walls opportunistically as the field pulls them).
pub(crate) const CRAB_WALL_CLUSTERS: usize = 2;
/// Nests spawn at least this far (tiles) from the squad spawn, and clusters at least this far apart.
pub(crate) const CRAB_MIN_SPAWN_DIST: f32 = 8.0;
pub(crate) const CRAB_CLUSTER_SEP: f32 = 5.0;

// Crab locomotion, boids, pounce, scout, feeding, and caste BEHAVIOUR constants moved to the `behavior:`
// config slice (`behavior.crab`, src/behavior_tuning.rs) so they are hand-tunable and searchable by
// `squad_ai::behavior_genome`. Only the render/collider/body-geometry constants stay here in code.

/// Uniform render scale for the child model (native height ~3.06 → ~0.46 m ≈ 1.5 ft tall, sized to
/// the ~6 ft squad and 8 ft ceilings). Seat constants below scale with it.
pub(crate) const CRAB_RENDER_SCALE: f32 = 0.15;
/// Root body-centre height above the surface, along the surface normal (also seats the collider).
pub(crate) const CRAB_BODY_CENTER: f32 = 0.125;
/// Local Y offset of the scaled model under the root so its body rests on the surface (the glb origin
/// sits near the model's top). Calibrated by eye via devshot, scaled with `CRAB_RENDER_SCALE`.
pub(crate) const CRAB_MODEL_Y: f32 = 0.275;
/// Radius of the invisible collider sphere (the laser raycast target); world-size since the root is
/// unscaled. Sized to hug the *visible* crab (rendered span ≈0.46 → radius ≈0.3) so a bolt only draws
/// blood on a real hit — a near-miss now passes cleanly instead of registering on an oversized hitbox.
/// Scales in lockstep with `CRAB_RENDER_SCALE` (2.5× when the model grew 0.06→0.15).
pub(crate) const CRAB_COLLIDER_R: f32 = 0.30;

/// The unit's body approximated as a vertical cylinder the crabs cling to (radius, climbable height).
pub(crate) const UNIT_BODY_RADIUS: f32 = 0.33;
pub(crate) const UNIT_BODY_HEIGHT: f32 = 1.0;
/// Reach the flow gate this close to commit the patch transfer.
pub(crate) const TRANSFER_RADIUS: f32 = 0.22;
/// How fast the crab's surface normal eases toward the new patch's normal on a transfer (per second).
pub(crate) const NORMAL_EASE: f32 = 12.0;
/// Frame-dt clamp so a hitch can't fling a crab off its surface (mirrors `enemy::MAX_FRAME_DT`).
pub(crate) const MAX_FRAME_DT: f32 = 1.0 / 30.0;
/// Cross-fade between animation clips.
pub(crate) const CROSSFADE: Duration = Duration::from_millis(150);
/// Clip playback-rate multipliers. The authored clips are extremely long (walk ≈ 10.5 s/loop, attack
/// ≈ 2.5 s), so at 1× the legs crawl through one cycle over many seconds — playing them several times
/// faster turns it into a frantic scuttle / rapid chomp. Tuned by eye.
pub(crate) const WALK_ANIM_SPEED: f32 = 7.0;
pub(crate) const ATTACK_ANIM_SPEED: f32 = 4.0;

pub(crate) const CRAB_GLB: &str = "dimensional_crab/dimensional_crab.glb";

/// Marker on a crab root entity (also the raycast collider).
#[derive(Component)]
pub struct Crab;

/// A crab's position on the surface manifold and its facing/seed state.
#[derive(Component)]
pub(crate) struct CrabMotion {
    /// Current [`SurfaceGraph`] patch id.
    patch: u32,
    /// World-space point ON the current surface (pre-seat).
    pos: Vec3,
    /// Current surface normal (eased toward the patch normal across transfers).
    normal: Vec3,
    /// Last travel heading (for smooth facing).
    heading: Vec3,
    /// Per-crab preferred height fraction `[0,1]` on a unit's body when latched, so the swarm spreads
    /// over the whole body (piranha) instead of piling at the feet.
    climb_bias: f32,
    /// Per-crab preferred angular slot `[0,1]` around the body (mapped across `bc.back_spread`).
    angle_bias: f32,
    /// Body-relative angle (radians) of the crab's slot around its host, `0` = dead-centre back. Set
    /// once on latching so the crab holds that spot and rides along as the host turns and walks.
    latch_rel: f32,
}

/// Public link from a latched crab to the unit it's feeding on (`None` when free-roaming). Read by the
/// laser so a bolt that shoots this crab off can roll to also wound its host (friendly fire).
#[derive(Component)]
pub struct CrabAttached {
    pub host: Option<Entity>,
}

/// A crab's foraging/carrying state: how much it can lift, the specific gib it's committed to (a
/// `Vec3`-only `ActiveBehavior.target` can't hold an entity), and whether its gib is currently lifted.
#[derive(Component)]
pub struct CrabCarry {
    pub capacity: f32,
    pub target: Option<Entity>,
    pub hauling: bool,
}

/// Animation state, chosen from movement/contact each frame.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CrabState {
    Idle,
    Walk,
    Attack,
}

/// A crab's pounce state: crabs lunge ~`bc.jump_len` (≈10 body lengths) at a nearby unit, hunkering down
/// briefly before launching on a ballistic arc that bites on landing. `Ready` = grounded (normal
/// locomotion runs); `Hunker`/`Air` = the jump owns the crab's transform, so `crab_locomotion` skips it.
#[derive(Component)]
pub(crate) struct CrabJump {
    phase: JumpPhase,
    /// Time left in the current `Hunker`/`Air` phase.
    timer: f32,
    /// Cooldown before the next pounce (counts down while `Ready`).
    cooldown: f32,
    from: Vec3,
    to: Vec3,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum JumpPhase {
    Ready,
    Hunker,
    Air,
}

/// A scout's recon state machine: roam hunting for prey, then track a sighting and mark it with the
/// vectorial rally pheromone (Tang et al. 2019) so the assault swarm converges on the live prey.
#[derive(Clone, Copy)]
pub(crate) enum ScoutState {
    /// Ranging the map looking for prey (aggressive wander).
    Roaming,
    /// Tracking spotted prey — approaching `prey_pos` (refreshed each frame it still senses the prey) and
    /// depositing rally-pheromone vectors that point toward it.
    Tracking { prey_pos: Vec3 },
}

/// Marks the ~[`bc.scout_fraction`] of crabs that are scouts (recon recruiters). Holds the roam/track state
/// and this scout's private wander heading + RNG. Models ant scout-recruitment foraging by minimalist
/// agents (Talamali et al., Swarm Intelligence 2019, DOI 10.1007/s11721-019-00176-9): roam → spot →
/// track-and-mark → the swarm converges
/// via the vectorial rally pheromone (Tang et al. 2019). Read by `think` (to drive the Scout/Mark modes)
/// and `scout_mark_prey` (detection + rally deposit); its wander state is advanced by `crab_locomotion`.
#[derive(Component)]
pub struct Scout {
    state: ScoutState,
    /// Current roam heading (world dir, re-rolled on `wander_timer`); mirrors `EnemyMotion.wander_dir`.
    wander_dir: Vec3,
    wander_timer: f32,
    /// Throttle between successive rally-pheromone deposits (seconds). Keeps a marking scout from
    /// saturating a cell every frame while it tracks the prey; the pheromone's own decay handles call-off.
    report_cooldown: f32,
    /// Per-scout LCG state for heading re-rolls (seeded from spawn hash so scouts diverge).
    rng: u32,
}

impl Scout {
    fn new(rand_seed: u32) -> Self {
        Self {
            state: ScoutState::Roaming,
            wander_dir: Vec3::ZERO,
            wander_timer: 0.0,
            report_cooldown: 0.0,
            // Salt distinct from the spawn-bundle draws so a scout's roam RNG is independent of its role
            // draw (see `CrabSpawnSeq`); `| 1` keeps the LCG state odd/non-zero.
            rng: (hash01_u32(rand_seed.wrapping_mul(0x9E37_79B1).wrapping_add(11)) * 4_000_000.0) as u32
                | 1,
        }
    }

    /// 1.0-equivalent gate for the brain: this scout is tracking a live sighting (latches the Mark mode).
    pub fn prey_spotted(&self) -> bool {
        matches!(self.state, ScoutState::Tracking { .. })
    }

    /// The prey a marking scout is tracking (the Mark behaviour's aim point — it approaches this to keep
    /// the rally pheromone fresh toward the prey's current position).
    pub fn tracked_prey(&self) -> Option<Vec3> {
        match self.state {
            ScoutState::Tracking { prey_pos } => Some(prey_pos),
            ScoutState::Roaming => None,
        }
    }
}

/// Link from a crab to its (asynchronously-spawned) `AnimationPlayer`, plus which state's clip is
/// currently playing (so `drive_crab_animation` only re-triggers on a real change).
#[derive(Component)]
pub(crate) struct CrabAnimPlayer {
    player: Entity,
    playing: Option<CrabState>,
}

/// The shared surface pursuit field (analog of `enemy::EnemyField`): rebuilt only when the set of unit
/// cells changes, shared read-only by the whole swarm.
#[derive(Resource, Default)]
pub(crate) struct CrabField {
    field: Option<Arc<SurfaceField>>,
    last_cells: Vec<IVec2>,
}

/// Monotonic spawn counter — a unique, ever-increasing seed handed to each crab at birth. Per-crab
/// randomization (scout/assault role, think-stagger, jump cadence, carry capacity, climb/angle biases,
/// RNG) is derived from THIS, never from the spawn *position*: nest-bred crabs all seat on the one
/// delivery cell, so a position hash would make every sibling a byte-identical clone (collapsing the
/// scout split to per-nest all-or-nothing). One counter, incremented once per spawn, keeps them distinct.
#[derive(Resource, Default)]
pub(crate) struct CrabSpawnSeq(u64);

/// The one shared animation graph + node handles for the three crab clips.
#[derive(Resource)]
pub(crate) struct CrabAnim {
    graph: Handle<AnimationGraph>,
    idle: AnimationNodeIndex,
    walk: AnimationNodeIndex,
    attack: AnimationNodeIndex,
}

pub struct CrabPlugin;

impl Plugin for CrabPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CrabField>()
            .init_resource::<CrabSpawnSeq>()
            // Graph and anim resources must exist before crabs are spawned/snapped to patches.
            .add_systems(Startup, (build_surface_graph, build_crab_anim, spawn_crabs).chain())
            // Pinned crab simulation on `FixedUpdate` — locomotion, jumping, carry economy, combat,
            // deposits, and reproduction. All the `.after(AiSet::…)` / inter-system orderings stay valid
            // because `AiSet` and every one of these systems now live on `FixedUpdate` together.
            .add_systems(
                FixedUpdate,
                (
                    rebuild_crab_field,
                    // Enlist foraging crabs onto specific gibs before locomotion reads their targets.
                    assign_meat_targets
                        .after(crate::ai::AiSet::Think)
                        .before(crab_locomotion),
                    // A crab that left SeekMeat/Carry drops its load before the carry machine
                    // re-evaluates the crew (fleeing, latching, or re-foraging all release).
                    release_uncommitted_carriers
                        .after(crate::ai::AiSet::Think)
                        .before(carry_gibs),
                    // Move after the brain has chosen this tick's mode (see `crate::ai`).
                    crab_locomotion
                        .after(rebuild_crab_field)
                        .after(crate::ai::AiSet::Think)
                        // Read the light field the tick it was baked (mirrors the `fog::LosWritten` gate).
                        .after(crate::light::LightFieldWritten)
                        // Likewise read the Almond Water field the tick it was updated + drunk down, so the
                        // forage nudge steers on the current level (post-heal), not last tick's.
                        .after(crate::almond_water::AlmondWaterWritten),
                    // Pounce owns the transform of mid-jump crabs; runs after locomotion set the rest.
                    // Third link in the cross-plugin `HealthDamage` chain (see `health::HealthDamage`'s
                    // doc and `enemy::smiley_zap`'s registration comment).
                    crab_jump
                        .after(crab_locomotion)
                        .after(crate::enemy::smiley_defense)
                        .in_set(crate::health::HealthDamage),
                    // Cooperative lift/haul/deliver — runs after crabs have moved and any fleer released.
                    carry_gibs
                        .after(crab_locomotion)
                        .after(assign_meat_targets),
                    crab_contact_damage.after(crab_jump).in_set(crate::health::HealthDamage),
                    // Flood the local ALARM channel when a crab is wounded, before the deposits drain so
                    // the muster bloom is live this tick (mirrors `scout_mark_prey`'s ordering).
                    crab_alarm_on_damage.before(crate::ai::AiSet::Deposits),
                    // Sate HUNGER after the brain has consumed this tick's drive values.
                    crab_feeding_sates_hunger.after(crate::ai::AiSet::Think),
                    crab_despawn_dead.in_set(CrabDespawn),
                    deposit_crab_fields,
                    deposit_meat_scent,
                    // Scout detection + rally-pheromone deposit: before the rally deposits drain so the
                    // beacon is live this tick, and before Think so the tracking state feeds the scout brain.
                    scout_mark_prey.before(crate::ai::AiSet::Deposits),
                    // Dynamic castes: re-role scouts↔assault off the fresh fields, before the brains think.
                    re_role_crabs
                        .after(crate::ai::AiSet::FieldUpdate)
                        .before(crate::ai::AiSet::Think),
                    nest_reproduce,
                ),
            )
            // Cosmetic: skeletal animation attach/drive stays on `Update`.
            .add_systems(Update, (attach_crab_animation, drive_crab_animation));
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::dungeon::{Dungeon, TILE_SIZE, WALL_THICKNESS};
    use crate::surface_nav::SurfaceGraph;
    use crate::wfc::{E, N, S, W};

    #[test]
    fn caste_policy_promotes_and_demotes_on_the_right_signals() {
        // The caste thresholds are now config (behavior.crab); test against the shipped defaults.
        let bc = crate::behavior_tuning::BehaviorTuning::default().crab;
        let (ah, pd) = (bc.alarm_high, bc.promote_density);
        // Assault crab, crowded, no beacon → promote to scout (spare a body for recon).
        assert_eq!(
            caste_decision(false, false, pd + 1.0, 0.0, ah, pd),
            Rerole::Promote
        );
        // Assault crab but a beacon is live → hold (the swarm is already converging; stay a fighter).
        assert_eq!(
            caste_decision(false, true, pd + 1.0, 0.0, ah, pd),
            Rerole::Hold
        );
        // Assault crab, uncrowded → hold.
        assert_eq!(caste_decision(false, false, 0.0, 0.0, ah, pd), Rerole::Hold);
        // Scout with a live beacon → demote (recon done, become a fighter).
        assert_eq!(caste_decision(true, true, 0.0, 0.0, ah, pd), Rerole::Demote);
        // Scout under alarm → demote (defense press).
        assert_eq!(caste_decision(true, false, 0.0, ah + 0.1, ah, pd), Rerole::Demote);
        // Scout, calm, no beacon → hold (keep ranging).
        assert_eq!(caste_decision(true, false, 0.0, 0.0, ah, pd), Rerole::Hold);
    }

    /// The wall's inner (room-facing) face, from cell centre — the ground-truth threshold
    /// `Dungeon::is_solid` uses. A crab body must stay short of this by its radius.
    fn wall_inner_face() -> f32 {
        0.5 * TILE_SIZE - WALL_THICKNESS
    }

    /// Build a `Dungeon` whose only floor cells are `floors` (all else rock), on a `w×h` grid.
    fn dungeon_with(w: usize, h: usize, floors: &[IVec2]) -> Dungeon {
        let mut mask = vec![false; w * h];
        for c in floors {
            mask[c.y as usize * w + c.x as usize] = true;
        }
        Dungeon::from_walkable(w, h, mask)
    }

    /// A crab clamped onto a floor patch — even shoved hard past every walled edge — must never
    /// end up with its body inside a bounding wall slab. This is the exact bug that was reported
    /// (crabs clipping into walls) and the invariant the per-edge floor-patch inset restores.
    #[test]
    fn crab_cannot_be_clamped_into_a_wall() {
        // A single floor cell at (1,1) surrounded by rock ⇒ walled on all four edges.
        let cell = IVec2::new(1, 1);
        let dungeon = dungeon_with(3, 3, &[cell]);
        for dir in [N, E, S, W] {
            assert!(dungeon.walled(cell, dir), "fixture must wall every edge of the cell");
        }

        let graph = SurfaceGraph::build(&dungeon);
        let patch_id = graph
            .floor_patch_cell(cell)
            .expect("center cell must have a floor patch");
        let patch = graph.patch(patch_id);
        let center = dungeon.cell_center(cell);
        let r = CRAB_COLLIDER_R;

        // Sanity: the walls really are there — the FULL-tile corner (what the old, un-inset clamp
        // permitted) sits inside a wall slab. This is precisely what used to let crabs clip.
        assert!(
            dungeon.is_solid_test(center.x + 0.5 * TILE_SIZE, center.z),
            "the tile edge is inside a wall — the old full-tile clamp would embed the crab here"
        );

        // Shove a crab far past each edge (and past each corner), clamp, and assert its whole
        // body footprint (centre ± radius on X and Z) stays out of solid geometry.
        let far = 5.0;
        let pushes = [
            Vec3::new(far, 0.0, 0.0),
            Vec3::new(-far, 0.0, 0.0),
            Vec3::new(0.0, 0.0, far),
            Vec3::new(0.0, 0.0, -far),
            Vec3::new(far, 0.0, far),
            Vec3::new(-far, 0.0, far),
            Vec3::new(far, 0.0, -far),
            Vec3::new(-far, 0.0, -far),
        ];
        for push in pushes {
            let clamped = clamp_to_patch(center + push, patch);
            for (dx, dz) in [(0.0, 0.0), (r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)] {
                assert!(
                    !dungeon.is_solid_test(clamped.x + dx, clamped.z + dz),
                    "crab body clipped a wall: push={push:?} clamped={clamped:?} offset=({dx},{dz})"
                );
            }
        }
    }

    /// The inset must apply ONLY to walled edges. An OPEN edge (floor neighbour) keeps the full
    /// half-tile so a crab can still reach the floor↔floor transfer gate at the cell boundary —
    /// insetting it would strand the crab and break pursuit. Uses a straight corridor so the
    /// middle cell is open E/W and walled N/S.
    #[test]
    fn open_edges_keep_full_extent_walled_edges_inset() {
        let cells = [IVec2::new(0, 1), IVec2::new(1, 1), IVec2::new(2, 1)];
        let mid = cells[1];
        let dungeon = dungeon_with(3, 3, &cells);
        assert!(!dungeon.walled(mid, E) && !dungeon.walled(mid, W), "E/W must be open");
        assert!(dungeon.walled(mid, N) && dungeon.walled(mid, S), "N/S must be walled");

        let graph = SurfaceGraph::build(&dungeon);
        let patch = graph.patch(graph.floor_patch_cell(mid).expect("floor patch"));
        let center = dungeon.cell_center(mid);

        // Open edges (±X): the crab reaches the full ±0.5 boundary (the transfer gate location).
        let east = clamp_to_patch(center + Vec3::new(5.0, 0.0, 0.0), patch);
        let west = clamp_to_patch(center + Vec3::new(-5.0, 0.0, 0.0), patch);
        assert!(
            (east.x - center.x - 0.5 * TILE_SIZE).abs() < 1e-5,
            "open E edge must keep full half-tile extent, got {}",
            east.x - center.x
        );
        assert!(
            (west.x - center.x + 0.5 * TILE_SIZE).abs() < 1e-5,
            "open W edge must keep full half-tile extent, got {}",
            west.x - center.x
        );

        // Walled edges (±Z): the crab is held short of the wall inner face by its radius.
        let bound = wall_inner_face() - CRAB_COLLIDER_R;
        let south = clamp_to_patch(center + Vec3::new(0.0, 0.0, 5.0), patch);
        let north = clamp_to_patch(center + Vec3::new(0.0, 0.0, -5.0), patch);
        assert!(
            (south.z - center.z - bound).abs() < 1e-5,
            "walled S edge must inset to inner_face - radius, got {}",
            south.z - center.z
        );
        assert!(
            (north.z - center.z + bound).abs() < 1e-5,
            "walled N edge must inset to inner_face - radius, got {}",
            north.z - center.z
        );
        // And that inset genuinely keeps the body off the slab.
        assert!(!dungeon.is_solid_test(south.x, south.z + CRAB_COLLIDER_R));
        assert!(!dungeon.is_solid_test(north.x, north.z - CRAB_COLLIDER_R));
    }
}
