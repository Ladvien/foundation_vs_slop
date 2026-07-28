//! SCP-999 — "The Tickle Monster": a friendly, amorphous orange gelatinous blob, the game's first
//! *benign* creature. Where the crab swarm, the smiley watcher, and the SCP-150 parasite all raise the
//! squad's **FEAR** drive (anxiety), SCP-999 *lowers* it: it oozes toward the most-frightened squad member
//! and, on contact, **tickles** them — draining their FEAR and lifting their MORALE, like a comforting
//! puppy. It has runtime soft-body jelly jiggle and two big glossy procedural eyes that track whoever it is
//! caring for.
//!
//! ## Grounding (home-still research)
//! - The tickle-calm is companion-animal **social buffering**: a friendly animal's presence lowers anxiety,
//!   and *contact* (petting/tickling a live animal) lowers it more than a toy — Shiloh et al. 2003, reviewed
//!   in Beetz et al. 2012 (Front. Psychol., "Psychosocial and psychophysiological effects of human-animal
//!   interactions"). FEAR is a high-arousal emotion that *spreads* between members (emotional contagion —
//!   Elfenbein 2023, Annu. Rev. Psychol.), so a calm-source counteracts the contagion at its target.
//! - Seek steering toward the target follows Reynolds, "Flocks, Herds and Schools" (SIGGRAPH 1987) /
//!   "Steering Behaviors For Autonomous Characters" (GDC 1999).
//! - The soft body is damped modal dynamics — see [`jiggle`] (Pentland & Williams 1989).
//!
//! ## Architecture (the determinism split — see `TESTING.md`)
//! Two plugins, two schedules:
//! - [`Scp999Plugin`] — the **gameplay**: spawn + seek-movement + tickle-contact calm. Runs on `FixedUpdate`
//!   `.after(AiSet::Think)`. The calm writes squad `Drives` (FEAR/MORALE), which feed Think → unit movement
//!   → hashed `Transform`, so this is pinned simulation and is registered in `sim_harness` too.
//! - [`Scp999VisualsPlugin`] — the **cosmetic** layer: the eye material + billboard, and the soft-body
//!   jiggle (writes only `MorphWeights`, never `(Transform, Health)`). Windowed-only, like `HairPlugin`;
//!   never in the deterministic core.
//!
//! Entity shape (root scale 1 so the eye billboard is sized in world units; the gel model is a scaled
//! child, exactly like `crate::crab`):
//!   root  = `Scp999` + `Scp999Motion` + [`jiggle::BlobJiggle`] + `Transform` (moves on the ground plane).
//!   child = `WorldAssetRoot` of `scp999/scp-999.glb#Scene0`, scaled to ~1 m.
//!   child = (windowed only) the eye billboard quad, added by [`eyes::attach_scp999_eyes`].
//!
//! Deliberately carries **no `Hostile`** (so the squad never lasers it and the fog never hides it), **no
//! `Prey`** (so crabs never swarm it), and **no `Drives`** (so it needs no `Faction`: `validate_factions`
//! only panics on a `Drives` carrier without one).

use bevy::prelude::*;

use crate::dungeon::Dungeon;
use crate::sim::SimTuning;

mod eyes;
mod jiggle;
mod movement;

pub(crate) use jiggle::BlobJiggle;

/// The self-contained gel asset (see `assets/scp999/README.md` for the verified contract).
pub(crate) const SCP999_GLB: &str = "scp999/scp-999.glb";
/// Render scale for the gel model child. The asset is authored large (~1.52 m tall, ~2.5 m wide) with its
/// base at `y = 0`. `0.45` gives a ~0.68 m mound (~1.1 m wide) — shrunk from the original ~1 m so its body
/// threads corridors far better while it still reads as a sizeable slime. (It's a heavily-deforming soft
/// body, so it can still *bulge* toward a wall — that's slime, not a clip; the wall-standoff push keeps its
/// centre clear.) Same "unscaled root, scaled model child" convention as the crab.
pub(crate) const RENDER_SCALE: f32 = 0.45;
/// Collision half-extents for wall-sliding ([`Dungeon::resolve_move`]). The gel is visually wide but a
/// friendly blob should thread the same corridors the squad does, so the collision box matches the
/// squad/enemy footprint (`0.27`..`0.3`) — the soft body harmlessly overlaps a wall it hugs, which reads
/// as a squishy blob, not a bug.
pub(crate) const SCP999_HALF: Vec2 = Vec2::splat(0.3);
/// The unit's body approximated as a cylinder for the tickle-contact reach (matches `crab::UNIT_BODY_RADIUS`).
pub(crate) const UNIT_BODY_RADIUS: f32 = 0.33;
/// Clamp per-frame dt so a hitch can't tunnel the blob through a wall (mirrors `enemy`/`crab` movement).
pub(crate) const MAX_FRAME_DT: f32 = 1.0 / 30.0;

/// Marker on an SCP-999 root entity.
#[derive(Component)]
pub struct Scp999;

/// The blob's current pursuit state, written each fixed tick by [`movement::scp999_seek_and_tickle`] and
/// read by the cosmetic eye/jiggle systems. `gaze` is the world point the eyes look toward (its current
/// target member); `tickling` is true the tick it is in contact and delivering comfort (drives the eye
/// delight + the jiggle bounce); `moving` is true while oozing.
#[derive(Component, Default)]
pub(crate) struct Scp999Motion {
    pub(crate) target: Option<Entity>,
    pub(crate) gaze: Vec3,
    pub(crate) moving: bool,
    pub(crate) tickling: bool,
}

/// Monotonic spawn counter — a unique, ever-increasing seed handed to each blob at birth (mirrors
/// `crab::CrabSpawnSeq`). Used only for cosmetic decorrelation (each blob's idle-breath + blink phase), so
/// several blobs don't pulse in lockstep. Never keyed on `Entity` (recycled ids break determinism) and
/// never used for a gameplay decision.
#[derive(Resource, Default)]
pub(crate) struct Scp999Seq(pub(crate) u32);

/// The blob's own copy of its birth seed, carried so every cosmetic decorrelation derives from the SAME
/// stable number [`Scp999Seq`] promises. The eye layer used to draw its blink phase from a `Local<u32>`
/// attach counter instead, which orders blobs by *asset-arrival* — a per-run quantity — so which blob
/// blinked on which phase changed between runs of one seed. Present on every blob (never a subset marker,
/// so no archetype split) and never read by a gameplay decision.
#[derive(Component)]
pub(crate) struct Scp999Seed(pub(crate) u32);

/// The gameplay half: spawn + seek + tickle-calm. Registered in BOTH `lib::run` (windowed) and
/// `sim_harness::build_headless_app` (headless), because the calm mutates squad `Drives`, which propagates
/// into the hashed simulation.
pub struct Scp999Plugin;

impl Plugin for Scp999Plugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Scp999Seq>()
            .add_systems(OnEnter(crate::session::RunState::Active), spawn_scp999.in_set(crate::session::RunBuild::Populate))
            // Move + calm after the brain has chosen this tick's modes (the calm writes FEAR/MORALE, which
            // next tick's Think reads). Pinned sim on `FixedUpdate`, like every creature system.
            .add_systems(
                FixedUpdate,
                movement::scp999_seek_and_tickle.after(crate::ai::AiSet::Think).distributive_run_if(in_state(crate::session::RunState::Active)),
            );
    }
}

/// The cosmetic half: the eye material + billboard, and the soft-body jiggle. Windowed-only (registered
/// ONLY in `lib::run`, never in `sim_harness`) — it writes `MorphWeights` and material uniforms and a
/// child billboard `Transform`, none of which is `(Transform, Health)` on a hashed entity, so it can never
/// perturb `snapshot_hash`. Same discipline as `HairPlugin`.
pub struct Scp999VisualsPlugin;

impl Plugin for Scp999VisualsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy::pbr::MaterialPlugin::<eyes::Scp999EyesMaterial>::default())
            .add_systems(
                Update,
                (
                    (eyes::attach_scp999_eyes, eyes::update_scp999_eyes).chain(),
                    // Fog of war hides the blob when it's outside the squad's live line of sight — the
                    // one shared `fog::hide_in_fog` pass, keyed on this marker (SCP-999 isn't `Hostile`,
                    // so it names itself). Cosmetic: Visibility is render state; gameplay runs regardless.
                    crate::fog::hide_in_fog::<Scp999>,
                ).distributive_run_if(in_state(crate::session::RunState::Active)),
            )
            // Reactive modal jiggle, written onto the gel's morph weights (see `jiggle`). `PostUpdate` so it
            // runs after gameplay movement set this tick's Transform (the springs read its acceleration).
            .add_systems(PostUpdate, jiggle::drive_blob_jiggle.distributive_run_if(in_state(crate::session::RunState::Active)));
    }
}

/// Minimum spacing between two comfort blobs, so a `count > 1` world scatters them instead of stacking
/// them in one room. Matches `enemy::SPAWN_SEP`; the distance from the *squad* is the evolvable
/// `scp999.spawn_min_dist`.
const SPAWN_SEP: f32 = 3.0;

/// Spawn `sim.scp999.count` blobs out in the level, at least `sim.scp999.spawn_min_dist` tiles from the
/// squad spawn and `SPAWN_SEP` apart from each other — the same greedy far-from-spawn placement the
/// enemies (`enemy::spawn_enemies`) and crab nests (`crab::setup::spawn_crabs`) use, so every creature
/// seeds by one rule. The blob used to fan out one tile behind the squad; starting in contact meant the
/// squad never actually carried its fear anywhere, and the player asked for it to seed out in the world
/// (debug capture 2026-07-24). Deterministic: a fixed raster scan, no RNG — so for a given level layout
/// the blob starts in the same place every run, and `spawn_min_dist` (evolvable) is what moves it.
/// Per-blob decorrelation comes from the monotonic [`Scp999Seq`].
fn spawn_scp999(
    mut commands: Commands,
    dungeon: Res<Dungeon>,
    sim: Res<SimTuning>,
    rules: Res<crate::containment::ContainmentRules>,
    assets: Res<AssetServer>,
    mut seq: ResMut<Scp999Seq>,
    mut targets: ResMut<crate::containment::TargetSeq>,
) {
    if sim.scp999.count == 0 {
        return;
    }
    let mut chosen: Vec<IVec2> = Vec::new();
    'scan: for y in 0..dungeon.height as i32 {
        for x in 0..dungeon.width as i32 {
            let cell = IVec2::new(x, y);
            if !dungeon.is_floor(cell) {
                continue;
            }
            if (cell - dungeon.spawn).as_vec2().length() < sim.scp999.spawn_min_dist {
                continue;
            }
            if chosen
                .iter()
                .any(|c| (*c - cell).as_vec2().length() < SPAWN_SEP)
            {
                continue;
            }
            chosen.push(cell);
            if chosen.len() >= sim.scp999.count {
                break 'scan;
            }
        }
    }

    if chosen.is_empty() {
        warn!(
            "scp999: no floor cell at least {} tiles from spawn — no comfort blob placed",
            sim.scp999.spawn_min_dist
        );
        return;
    }

    for cell in &chosen {
        let s = seq.0;
        seq.0 += 1;
        spawn_scp999_at(
            &mut commands,
            &assets,
            s,
            dungeon.cell_center(*cell),
            rules.0.scp999.clone(),
            &mut targets,
        );
    }
    info!("scp999: spawned {} comfort blob(s) out in the level", chosen.len());
}

/// Spawn one SCP-999 at `pos` with decorrelation seed `seed`. The single builder both the Startup spawner
/// and the Research Room dev-tool use, so an F6-dropped blob is byte-identical to a natural one.
pub fn spawn_scp999_at(
    commands: &mut Commands,
    assets: &AssetServer,
    seed: u32,
    pos: Vec3,
    rule: crate::containment::ContainmentRule,
    seq: &mut crate::containment::TargetSeq,
) -> Entity {
    commands
        .spawn((
            crate::session::run_scoped(),
            Scp999,
            // The uniform key the player's aim resolves by. Minted here, in the SHARED builder, so an
            // F6-dropped blob is byte-identical to a seeded one in this respect too.
            seq.next(),
            // **The tutorial capture** (FVS-C-2). The blob is contained by *befriending* it — holster
            // (let `THREAT_GUN` decay at its cell) and stay with it (keep `ATTENTION` on it) — not by
            // trapping it. Both clauses are things the player does by choosing NOT to fight, which
            // states the whole win-by-containing pivot in one creature. The rule is authored in the
            // `containment:` config slice; the state component rides here so a dev-spawned blob
            // (Research Room F6) is byte-identical to a naturally seeded one.
            crate::containment::Containment::new(rule, crate::knowledge::Subject::ComfortBlob),
            Scp999Seed(seed),
            Scp999Motion::default(),
            BlobJiggle::new(seed),
            // Root is unscaled (keeps the eye billboard in world units); the gel model child carries the
            // render scale. No spawn yaw — the blob is radially symmetric, so it has no "front" (README §2).
            Transform::from_translation(pos),
            Visibility::Inherited,
            // Render-only: smooth the blob's 60 Hz movement across the display refresh (see `lib::run`).
            avian3d::prelude::TransformInterpolation,
        ))
        .with_child((
            WorldAssetRoot(assets.load(GltfAssetLabel::Scene(0).from_asset(SCP999_GLB))),
            // Base rests at y=0 in the asset, so the scaled model sits on the floor with no Y offset.
            Transform::from_scale(Vec3::splat(RENDER_SCALE)),
        ))
        .id()
}


