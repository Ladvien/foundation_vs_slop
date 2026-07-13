//! Brain glue — builds each agent's [`Perception`], runs the utility [`decide`], and writes the chosen
//! [`ActiveBehavior`] (mode + target) that the wrapped locomotion systems execute. Decisions are
//! throttled by a per-agent [`ThinkTimer`] (seed-staggered) so 40+ agents don't all re-decide on the
//! same frame; movement runs every frame off the cached choice (Dill Ch.3: decide *at decision
//! points*, not every tick).
//!
//! Creature repertoires live in the `*_brain()` builders — each a data literal, the place a developer
//! adds/edits behaviours.

use std::sync::Arc;

use bevy::prelude::*;

use super::drives::{DriveId, Drives};
use super::field::{FieldId, Stig};
use super::utility::{
    decide, Behavior, Consideration, Curve, Fact, Input, Mode, Perception, SquadFields, TargetKind,
};
use crate::dungeon::Dungeon;
use crate::flowfield::FlowField;
use crate::health::Health;
use crate::squad::Unit;
use crate::util::hash01_u32;

/// A blood frenzy must reach this SCENT strength before the boss paths to it (matches the HuntBlood gate).
const HUNT_SCENT_MIN: f32 = 1.0;

/// The local vectorial-rally magnitude a crab must be standing in before it masses on the sighting (gates
/// the Rally behaviour, and gates Flee off, mirroring `HUNT_SCENT_MIN`). A cell a scout is actively
/// marking clears it; an evaporated / distant cell won't, so only crabs near the sighting are recruited.
const RALLY_MIN: f32 = 0.5;

/// The local ALARM strength a crab must be standing in before it musters on a wounded neighbour (gates the
/// Muster behaviour on, and gates Flee off — the retaliation twin of the `RALLY_MIN` / berserk gates). A
/// cell inside a fresh casualty's ~one-room bloom clears it; an evaporated / distant cell won't, so only
/// crabs near the wound surge while the rest of the swarm still fears gunfire.
const ALARM_MIN: f32 = 0.3;

/// Seconds between decisions for one agent (steering still runs every frame). RON-tunable later.
const THINK_INTERVAL: f32 = 0.3;
/// Boss "chase falls off" range, in tiles (mirrors `enemy::CHASE_TILES`).
const CHASE_TILES: f32 = 16.0;

/// A creature's full behaviour repertoire.
pub struct Brain {
    pub behaviors: Vec<Behavior>,
}

/// The brains, one per creature type, built at startup (the behaviour extension point).
#[derive(Resource)]
pub struct AiBrains {
    pub smiley: Brain,
    pub crab: Brain,
    pub scout: Brain,
}

/// Which brain an agent uses.
#[derive(Component, Clone, Copy)]
pub enum BrainId {
    Smiley,
    /// Attached to the ~80% assault crabs (the killing swarm).
    Crab,
    /// Attached to the ~20% of crabs that are scouts (roam/report recon).
    Scout,
}

/// The current decision, written by `think`, read by the locomotion systems each frame.
#[derive(Component)]
pub struct ActiveBehavior {
    pub mode: Mode,
    pub target: Option<Vec3>,
    pub rng: u32,
    /// Monotonic count of *actual* decisions — incremented once per `ThinkTimer` firing, in `think` and
    /// in `squad_ai::perception::squad_think`.
    ///
    /// Exists because Bevy's `Changed<ActiveBehavior>` **cannot** identify decision points for units:
    /// `squad_think` re-resolves `target` from fresh perception on *every* tick (so cohesion tracks the
    /// moving anchor), and that `DerefMut` marks the component changed 18× per decision at the 0.3 s
    /// think interval. A recorder keyed on `Changed` therefore oversampled the squad ~18× with ~94%
    /// self-transitions, which silently made `surprise::learnability` — whose whole job is to measure
    /// transition structure — read every brain as maximally predictable. Creatures were unaffected
    /// (`think` `continue`s before touching `active`), so the bug was invisible on the swarm side.
    ///
    /// `squad_ai::trace` samples on this counter advancing. Never reset.
    pub decision: u32,
}

impl ActiveBehavior {
    /// Seed the per-agent decision RNG from the unique spawn seed (NOT the spawn position — bred crabs
    /// share a cell, so a position hash would make every sibling decide identically). `| 1` keeps the
    /// LCG state odd/non-zero.
    pub fn new(rand_seed: u32) -> Self {
        Self {
            mode: Mode::Wander,
            target: None,
            rng: (hash01_u32(rand_seed.wrapping_mul(0x9E37_79B1).wrapping_add(7)) * 4_000_000.0) as u32
                | 1,
            decision: 0,
        }
    }
}

/// Per-agent decision throttle (counts down to the next think).
#[derive(Component)]
pub struct ThinkTimer(pub f32);

impl ThinkTimer {
    /// Stagger the first think across a fresh cluster so siblings don't all think on the same frame —
    /// derived from the spawn seed (a distinct salt from the decision RNG).
    pub fn staggered(rand_seed: u32) -> Self {
        ThinkTimer(hash01_u32(rand_seed.wrapping_mul(0x9E37_79B1).wrapping_add(9)) * THINK_INTERVAL)
    }
}

/// The peak of a field each frame — a shared, computed-once signal (Mark Ch.30) so agents read a
/// global "where's the frenzy" without each scanning the grid.
#[derive(Resource, Default)]
pub struct FieldHotspots {
    pub scent: (Vec3, f32),
    pub meat: (Vec3, f32),
    /// Peak of the squad's audible din (`NOISE_SQUAD`) — where a firefight is loudest. The aim point for
    /// the crab `Investigate` behaviour (the swarm converging on the sound of the guns).
    pub noise_squad: (Vec3, f32),
}

/// Recompute the field hotspots once per frame (runs in `AiSet::FieldUpdate`). RALLY has no global peak
/// — it is a vectorial pheromone read locally per-crab (see [`crate::ai::field::RallyField`]).
pub fn update_hotspots(mut hot: ResMut<FieldHotspots>, stig: Option<Res<Stig>>, dungeon: Res<Dungeon>) {
    if let Some(stig) = stig {
        hot.scent = stig.hotspot(FieldId::SCENT, &dungeon);
        hot.meat = stig.hotspot(FieldId::MEAT, &dungeon);
        hot.noise_squad = stig.hotspot(FieldId::NOISE_SQUAD, &dungeon);
    }
}

/// A wall-aware pursuit field toward the current blood frenzy — so "drawn to blood" actually *paths*
/// there instead of straight-lining into walls. Mirrors `enemy::EnemyField`: one flow field seeded at
/// the scent-hotspot cell, rebuilt only when that cell changes.
#[derive(Resource, Default)]
pub struct ScentNav {
    pub field: Option<Arc<FlowField>>,
    last_cell: IVec2,
    active: bool,
}

/// Rebuild the scent pursuit field when the frenzy moves cells (runs in `AiSet::FieldUpdate`).
pub fn rebuild_scent_nav(mut nav: ResMut<ScentNav>, hot: Res<FieldHotspots>, dungeon: Res<Dungeon>) {
    let (pos, val) = hot.scent;
    if val < HUNT_SCENT_MIN {
        nav.field = None;
        nav.active = false;
        return;
    }
    let cell = dungeon.world_to_cell(pos);
    if nav.active && cell == nav.last_cell {
        return;
    }
    nav.field = FlowField::build_from(&dungeon, &[cell]).map(Arc::new);
    nav.last_cell = cell;
    nav.active = true;
}

/// Build each agent's perception, decide, and cache the choice. Throttled per agent.
#[allow(clippy::type_complexity)]
pub fn think(
    time: Res<Time>,
    stig: Res<Stig>,
    rally: Res<crate::ai::field::RallyField>,
    dungeon: Res<Dungeon>,
    hotspots: Res<FieldHotspots>,
    brains: Res<AiBrains>,
    fog: Res<crate::fog::FogGrid>,
    // The `audio:` slice — used to gate + scale the acoustic-din draw that latches `Mode::Investigate`.
    audio: Res<crate::audio_tuning::AudioTuning>,
    units: Query<&Transform, With<Unit>>,
    // Prey = units + the smiley boss. Crabs hunt any prey (nearest wins); the boss hunts only units
    // (it is Prey itself, so scanning Prey would make it target its own position).
    prey: Query<&Transform, With<crate::squad::Prey>>,
    mut agents: Query<
        (
            &Transform,
            &BrainId,
            &Drives,
            &Health,
            &mut ActiveBehavior,
            &mut ThinkTimer,
            Option<&crate::crab::CrabCarry>,
            Option<&crate::crab::Scout>,
        ),
        Without<Unit>,
    >,
) {
    let dt = time.delta_secs();
    for (tf, brain_id, drives, health, mut active, mut timer, carry, scout) in &mut agents {
        timer.0 -= dt;
        if timer.0 > 0.0 {
            continue;
        }
        timer.0 = THINK_INTERVAL;
        active.decision = active.decision.wrapping_add(1);

        let pos = tf.translation;
        // Nearest target via the shared ranking. Crabs and scouts scan all prey (units + boss); the boss
        // scans only units (never itself). The `999.0` sentinel (a "no target, effectively infinite
        // distance" signal for the distance-falloff curves) is applied here on the None arm.
        let hit = match brain_id {
            BrainId::Crab | BrainId::Scout => {
                crate::util::nearest_planar(pos, prey.iter().map(|t| ((), t.translation)))
            }
            BrainId::Smiley => {
                crate::util::nearest_planar(pos, units.iter().map(|t| ((), t.translation)))
            }
        };
        let (nearest_unit, nearest_dist) = match hit {
            Some(((), tpos, d)) => (Some(tpos), d),
            None => (None, 999.0),
        };

        let perc = Perception {
            pos,
            nearest_unit,
            nearest_dist,
            health_frac: if health.max > 0.0 {
                (health.current / health.max).clamp(0.0, 1.0)
            } else {
                0.0
            },
            drives: drives.v,
            scent_hotspot: hotspots.scent.0,
            scent_val: hotspots.scent.1,
            meat_hotspot: hotspots.meat.0,
            meat_val: hotspots.meat.1,
            carrying: if carry.is_some_and(|c| c.hauling) {
                1.0
            } else {
                0.0
            },
            prey_spotted: if scout.is_some_and(|s| s.prey_spotted()) {
                1.0
            } else {
                0.0
            },
            // LOCAL magnitude of the vectorial rally pheromone at this crab's cell (not a global peak):
            // only crabs actually near a scout-marked sighting rally / have their flight suppressed.
            rally_val: rally.sample(&dungeon, pos).length(),
            // LOCAL ALARM at this crab's cell — the "wounded kin" warning cry. Only crabs within ~one room
            // of a casualty read it, so only they muster and press through fire (see `Fact::AlarmHere`).
            alarm_val: stig.sample(FieldId::ALARM, &dungeon, pos),
            // Is this agent in the squad's live LOS? The boss's "pursue whenever seen, at any range" term
            // (see `Fact::SeenBySquad` / `smiley_brain`) — restores the aggro leash the seek rewrite dropped.
            seen_by_squad: if fog.visible_at(dungeon.world_to_cell(pos)) {
                1.0
            } else {
                0.0
            },
            // The squad's audible din, gated by `investigate_threshold` and scaled by `crab_draw_to_din`
            // (both from the `audio:` slice). Uses the GLOBAL `NOISE_SQUAD` peak (like MeatHotspot/
            // ScentHotspot), so the whole swarm shares one "where's the fight" pull. 0.0 unless the audio
            // search raised `crab_draw_to_din` — so `Mode::Investigate` is dormant at the shipped config.
            noise_draw: {
                let peak = hotspots.noise_squad.1;
                if peak >= audio.perception.investigate_threshold {
                    peak * audio.perception.crab_draw_to_din
                } else {
                    0.0
                }
            },
            // Crabs and the boss have no squad context — neutral unit fields (the squad brains never
            // run here; `think` is `Without<Unit>`).
            squad: SquadFields::neutral(),
        };

        let brain = match brain_id {
            BrainId::Smiley => &brains.smiley,
            BrainId::Crab => &brains.crab,
            BrainId::Scout => &brains.scout,
        };
        let idx = decide(&brain.behaviors, &perc, &mut active.rng);
        let chosen = &brain.behaviors[idx];
        active.mode = chosen.mode;
        active.target = match chosen.target {
            TargetKind::None => None,
            TargetKind::NearestUnit => nearest_unit,
            TargetKind::ScentHotspot => Some(hotspots.scent.0),
            TargetKind::MeatHotspot => Some(hotspots.meat.0),
            TargetKind::NoiseHotspot => Some(hotspots.noise_squad.0),
            // The Carry destination (the nest) is resolved per-crab by `carry_gibs` from the lifted
            // gib, not from the global hotspot — decide() only picks the mode here.
            TargetKind::Nest => None,
            // A marking scout aims at the live prey it is tracking (to keep laying the rally pheromone
            // toward its current position). Rally movement itself reads the local vector in `crab_rally`,
            // so the Rally behaviour needs no aim point here.
            TargetKind::TrackedPrey => scout.and_then(|s| s.tracked_prey()),
            // Squad-unit targets are resolved by `squad_ai::squad_think`, never by a crab/boss brain.
            TargetKind::SquadAnchor
            | TargetKind::NearestExaminable
            | TargetKind::NearestWoundedAlly
            | TargetKind::TrackedThreat => None,
        };
    }
}

/// The code-literal creature repertoires — the *templates* the offline behaviour search mutates, and the
/// reference brain whose realised mode distribution becomes the player's baseline expectation
/// (`squad_ai::surprise::ModePrior`).
pub fn authored_brains() -> AiBrains {
    AiBrains { smiley: smiley_brain(), crab: crab_brain(), scout: scout_brain() }
}

/// Where a repertoire comes from. **Always present** (`AiPlugin` `init_resource`s the `Authored`
/// default), so this is a parameter, not a fallback: the shipped game and a headless evaluation take the
/// same one code path with different data, and the match below is exhaustive.
///
/// The offline behaviour search (`squad_ai::genome`) mutates the *authored* repertoires and needs to run
/// the real simulation on the result. Rather than a second policy implementation — which would fork the
/// decision layer and lose the startup guards — a candidate simply replaces the brain data that
/// `utility::decide` already scores.
#[derive(Resource, Clone, Default)]
pub enum BrainSource {
    /// The code-literal defaults, plus any `assets/config/roles.ron` overlay (the shipped game).
    #[default]
    Authored,
    /// A candidate produced by the offline search. Every repertoire is validated below exactly as the
    /// authored ones are — an infeasible candidate is a loud failure, never a degraded brain.
    Candidate(Box<CandidateBrains>),
}

/// One point in the joint squad × swarm behaviour space: a full repertoire for every role and every
/// creature. Both sides are carried together because they co-adapt, and an evaluation of one is
/// meaningless without pinning the other.
#[derive(Clone)]
pub struct CandidateBrains {
    pub roles: std::collections::HashMap<crate::squad_ai::role::RoleId, Vec<Behavior>>,
    pub crab: Vec<Behavior>,
    pub scout: Vec<Behavior>,
    pub smiley: Vec<Behavior>,
}

/// Insert the creature brain registry (the developer's behaviour catalogue), or the candidate under
/// evaluation. Validation is shared: whichever source supplied them, every creature brain must keep an
/// unconditional low-rank default (Wander/Forage) or `decide` would have no eligible behaviour.
///
/// Creature brains deliberately **share ranks** (`Chase` and `HuntBlood` tie so the stronger pull wins;
/// so do `Latch` and `SeekMeat`), which reads as swarm variety rather than as thrash. So
/// `validate_rank_ladder` — a *role* invariant — is correctly not applied here.
pub fn init_brains(mut commands: Commands, source: Res<BrainSource>) {
    let brains = load_creature_brains(&source).unwrap_or_else(|e| panic!("creatures.ron: {e}"));
    commands.insert_resource(brains);
}

/// Which creature a `creatures.ron` entry overrides — the swarm analogue of `squad_ai::role::RoleId`.
/// `Deserialize`/`Hash`/`Eq` so it keys the override map; a key that is not one of these is a loud RON
/// parse error, never a silently skipped entry.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, serde::Deserialize)]
pub enum CreatureId {
    Smiley,
    Crab,
    Scout,
}

/// One creature's overridden repertoire (mirrors `squad_ai::role::RoleDef`). `Behavior` is already
/// `Deserialize`, so an evolved elite round-trips into a readable RON diff a designer can inspect.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct CreatureDef {
    pub behaviors: Vec<Behavior>,
}

/// Build the creature registry: the code-literal defaults (or a search candidate), overlaid by a validated
/// `assets/config/creatures.ron` when present. This is the SWARM-side promotion path, symmetric with
/// `squad_ai::load_role_brains` for the squad — so the offline search's evolved swarm repertoires
/// (`elites_swarm.ron`) can reach the shipped game through a DELIBERATE, reviewed act (transcribe a chosen
/// archive cell's crab/scout/smiley behaviours into `creatures.ron`), instead of the pipeline dead-ending
/// with no runtime reader at all. Promoting one is a deliberate change with its own golden re-pin.
///
/// One path, no fallback (mirrors `load_role_brains`): a MISSING file is the normal shipped case (the
/// code-literal defaults are a complete, playable swarm); a PRESENT-but-malformed/invalid file is a loud
/// startup panic, never a silent discard of the author's override. A search `Candidate` replaces the
/// repertoires wholesale and never overlays the file (an evaluation must run exactly the proposed brain).
fn load_creature_brains(source: &BrainSource) -> Result<AiBrains, String> {
    if let BrainSource::Candidate(candidate) = source {
        return validated_creatures(AiBrains {
            smiley: Brain { behaviors: candidate.smiley.clone() },
            crab: Brain { behaviors: candidate.crab.clone() },
            scout: Brain { behaviors: candidate.scout.clone() },
        });
    }

    let mut brains = authored_brains();
    match std::fs::read_to_string("assets/config/creatures.ron") {
        Ok(src) => {
            let defs: std::collections::HashMap<CreatureId, CreatureDef> =
                ron::from_str(&src).map_err(|e| format!("malformed: {e}"))?;
            for (id, def) in defs {
                let slot = match id {
                    CreatureId::Smiley => &mut brains.smiley,
                    CreatureId::Crab => &mut brains.crab,
                    CreatureId::Scout => &mut brains.scout,
                };
                *slot = Brain { behaviors: def.behaviors };
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("unreadable: {e}")),
    }
    validated_creatures(brains)
}

/// The one gate every creature brain passes through — a default, a `creatures.ron` override, or a search
/// candidate alike. Creatures deliberately SHARE ranks (Chase/HuntBlood tie so the stronger pull wins), so
/// the *role* rank-ladder invariant is correctly NOT applied here; only the unconditional-default check is
/// (without it, `decide` would find no eligible behaviour and the creature would freeze).
fn validated_creatures(brains: AiBrains) -> Result<AiBrains, String> {
    for (who, brain) in [
        ("smiley_brain", &brains.smiley),
        ("crab_brain", &brains.crab),
        ("scout_brain", &brains.scout),
    ] {
        crate::ai::utility::validate_unconditional_default(&brain.behaviors, who)?;
    }
    Ok(brains)
}

/// The smiley boss: hunt the nearest unit, but be **drawn to the biggest blood frenzy** (it reads the
/// shared SCENT field), else drift. Chase vs HuntBlood share a rank so the stronger pull wins.
fn smiley_brain() -> Brain {
    Brain {
        behaviors: vec![
            Behavior {
                mode: Mode::Chase,
                rank: 1,
                target: TargetKind::NearestUnit,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::NearestUnitDist),
                    curve: Curve::Linear {
                        m: -1.0 / CHASE_TILES,
                        b: 1.0,
                    },
                }],
            },
            // Aggro leash by LINE OF SIGHT, not just distance. A slow boss (MAX_SPEED < a unit) shot from
            // across the room is otherwise un-pursued: past CHASE_TILES the distance-Chase above scores 0
            // and decide() falls to Wander, so it drifts while being plinked — exactly the "walk up and it
            // ignores you" complaint. This twin Chase (same rank/target) fires whenever the boss stands in
            // the squad's live LOS, so being seen ALWAYS forces pursuit at any range (the OR the seek
            // rewrite dropped when it deleted `d <= CHASE_TILES || fog.visible_at(cell)`). Fuzzy-LOS aggro
            // is standard for partially-observable RTS AI (Yang, Xie & Peng, IEEE Access 2019).
            Behavior {
                mode: Mode::Chase,
                rank: 1,
                target: TargetKind::NearestUnit,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::SeenBySquad),
                    curve: Curve::Step {
                        threshold: 0.5,
                        below: 0.0,
                        above: 1.0,
                    },
                }],
            },
            Behavior {
                mode: Mode::HuntBlood,
                rank: 1,
                target: TargetKind::ScentHotspot,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::ScentHotspot),
                    // Gate on a real frenzy — near-zero scent must NOT win over chase/wander.
                    curve: Curve::Step {
                        threshold: 1.0,
                        below: 0.0,
                        above: 1.0,
                    },
                }],
            },
            Behavior {
                mode: Mode::Wander,
                rank: 0,
                target: TargetKind::None,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::SelfHealthFrac),
                    curve: Curve::Linear { m: 0.0, b: 0.15 }, // constant low default
                }],
            },
        ],
    }
}

/// The crab swarm brain — the emergent **frenzy → scatter** story lives in the rank ordering:
/// Flee(4) > Carry(3) > Latch/SeekMeat(2) > Rally(1) > Forage(0). A pursuing crab latches onto a unit
/// and feeds; but when the shared THREAT field spikes (gunfire), its FEAR climbs and the top-rank Flee
/// outranks everything, so the swarm scatters — then FEAR decays as THREAT evaporates and foraging
/// resumes. Nobody scripted "scatter". Rally sits *below* the attack: a scout's beacon redirects a
/// far/idle crab toward the sighting (beats plain Forage), but the moment it reaches a unit, Latch (and
/// the pounce) take over — so the recruited swarm actually *bites* instead of milling on the beacon cell.
/// A live rally *also* gates Flee off (like the ALARM muster gate), so a recruited swarm presses through
/// gunfire to the sighting; fear resumes once the beacon evaporates.
///
/// **Muster** (rank 1, Rally's twin) closes the "shoot the crabs and they just run" gap: when a crab is
/// wounded it floods a *local* ALARM pheromone (see [`FieldId::ALARM`] / `crab::crab_alarm_on_damage`), and
/// every crab within ~one room reads `Fact::AlarmHere` — which fires Muster (converge on the squad) AND
/// gates its Flee off. So a bolt into a pack no longer scatters it; the neighbours boil toward the shooter
/// and press until the alarm evaporates. Alarm-pheromone recruitment to defense in social insects — a
/// stigmergic warning cry (Heylighen, "Stigmergy as a universal coordination mechanism", CSR 2016).
fn crab_brain() -> Brain {
    Brain {
        behaviors: vec![
            // Default: pursue the squad across floor + walls. Always available (hunger-weighted).
            Behavior {
                mode: Mode::Forage,
                rank: 0,
                target: TargetKind::NearestUnit,
                considerations: vec![Consideration {
                    input: Input::Drive(DriveId::HUNGER),
                    curve: Curve::Linear { m: 0.8, b: 0.2 }, // always ≥0.2 so a choice exists
                }],
            },
            // Muster: a neighbour was just wounded — converge on the squad and press. Gated on the LOCAL
            // alarm bloom, so only crabs within ~one room of the casualty surge. Rank 3 so the alarm is
            // *decisive*: it beats foraging AND scavenging (Forage 0 / SeekMeat 2), so an alarmed crab
            // drops the food and charges the squad — the "converge + swarm the shooter" the design calls
            // for, not a leisurely graze. It still yields to the bite: a second Step gates Muster OFF once
            // the crab is within latch range (dist < ~LATCH_RANGE), so Latch (rank 2, un-suppressed once
            // Flee is alarm-gated) takes over and it feeds instead of charging through. Flee (rank 4) is
            // separately gated off by the same alarm (see below), so muster genuinely overrides flight.
            // Self-limiting — the alarm evaporates, both Steps fall through, and the crab reverts to
            // ordinary foraging (or fear). A stigmergic warning-cry recruitment (Heylighen, CSR 2016).
            Behavior {
                mode: Mode::Muster,
                rank: 3,
                target: TargetKind::NearestUnit,
                considerations: vec![
                    Consideration {
                        input: Input::Perc(Fact::AlarmHere),
                        curve: Curve::Step {
                            threshold: ALARM_MIN,
                            below: 0.0,
                            above: 1.0,
                        },
                    },
                    Consideration {
                        // Only while OUT of latch range — inside it, hand off to Latch so the crab bites.
                        input: Input::Perc(Fact::NearestUnitDist),
                        curve: Curve::Step {
                            threshold: 1.2, // ≈ LATCH_RANGE (matches the Latch gate)
                            below: 0.0,
                            above: 1.0,
                        },
                    },
                ],
            },
            // Climb onto and feed on a unit once close AND hungry.
            Behavior {
                mode: Mode::Latch,
                rank: 2,
                target: TargetKind::NearestUnit,
                considerations: vec![
                    Consideration {
                        input: Input::Perc(Fact::NearestUnitDist),
                        curve: Curve::Step {
                            threshold: 1.2, // ≈ LATCH_RANGE
                            below: 1.0,
                            above: 0.0,
                        },
                    },
                    Consideration {
                        input: Input::Drive(DriveId::HUNGER),
                        curve: Curve::Linear { m: 1.0, b: 0.2 },
                    },
                ],
            },
            // Scavenge meat: head for a pile when one exists and the crab is hungry. Same rank as
            // Latch — the crab does whichever its perception + drives weight higher this tick.
            Behavior {
                mode: Mode::SeekMeat,
                rank: 2,
                target: TargetKind::MeatHotspot,
                considerations: vec![
                    Consideration {
                        input: Input::Perc(Fact::MeatHotspot),
                        // MEAT peaks ≈0.25 at a fresh pile; scale so a real pile clears MIN_SCORE.
                        curve: Curve::Linear { m: 4.0, b: 0.0 },
                    },
                    Consideration {
                        input: Input::Drive(DriveId::HUNGER),
                        curve: Curve::Linear { m: 1.0, b: 0.2 },
                    },
                ],
            },
            // Haul a lifted gib home. Latches on `carrying` (set by `carry_gibs` on lift) and outranks
            // foraging/feeding so a laden crab commits to delivery until it drops or arrives.
            Behavior {
                mode: Mode::Carry,
                rank: 3,
                target: TargetKind::Nest,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::CarryingMeat),
                    curve: Curve::Step {
                        threshold: 0.5,
                        below: 0.0,
                        above: 1.0,
                    },
                }],
            },
            // Recruited surge: a scout is marking a prey sighting with the vectorial rally pheromone. Rank
            // 1 sits just above plain Forage, so it redirects a nearby crab up the rally vector toward the
            // sighting (a "warning-cry" recruitment; Heylighen, "Stigmergy as a universal coordination
            // mechanism", Cognitive Systems Research 2016) — but it yields to Latch/SeekMeat/Carry/Flee, so
            // a crab that reaches a unit bites (and pounces) instead of milling. Gated on the LOCAL rally
            // magnitude, so only crabs actually near a marked sighting rally; steering reads the local
            // vector in `crab_rally`, so no aim point is needed here. Self-limiting: the pheromone
            // evaporates, the Step falls below MIN_SCORE, and the crab drops back to plain foraging.
            Behavior {
                mode: Mode::Rally,
                rank: 1,
                target: TargetKind::None,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::RallyHere),
                    // Gate on a real local beacon — a faded/absent rally must NOT win over flee/forage.
                    curve: Curve::Step {
                        threshold: RALLY_MIN,
                        below: 0.0,
                        above: 1.0,
                    },
                }],
            },
            // Investigate: drawn toward the SOUND of the squad's guns (`NOISE_SQUAD`), the swarm
            // converging on a firefight. Rank 2, the forage/engage tier: it competes BY SCORE with
            // SeekMeat/Latch, so a loud-enough draw pulls a foraging crab off its meat and toward the din,
            // while a crab already latched onto a unit (high Latch score) keeps biting. Rank 1 was a dead
            // lever — the din only exists where the fight is, and there the rank-2 SeekMeat/Latch always
            // preempted a rank-1 Investigate, so it could never fire (measured: 0 investigate decisions even
            // at max draw). It still yields to Flee (rank 4): a crab fleeing gunfire does not turn to face
            // it. Steering follows the din hotspot in `crab_movement`. DORMANT at the shipped config:
            // `Fact::NoiseDraw` is `peak · crab_draw_to_din` (gated by `investigate_threshold` in `think`),
            // and `crab_draw_to_din` defaults to 0 — so this never clears MIN_SCORE until the audio search
            // raises it. That knob IS the emergent question "does the swarm run from the guns, or toward
            // them?". Self-limiting: the din evaporates, the score falls below MIN_SCORE, the crab reverts.
            Behavior {
                mode: Mode::Investigate,
                rank: 2,
                target: TargetKind::NoiseHotspot,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::NoiseDraw),
                    // Score IS the config-scaled draw; a plain pass-through so a louder fight / larger
                    // draw gain pulls harder. 0 (the shipped default) → ineligible.
                    curve: Curve::Linear { m: 1.0, b: 0.0 },
                }],
            },
            // Panic: flee down the THREAT gradient when afraid — outranks everything (drops the load).
            // Suppressed locally by a live rally beacon OR a nearby alarm bloom (a scout sighting or a
            // wounded neighbour / sieged nest): the recruited swarm presses the squad instead of fleeing.
            Behavior {
                mode: Mode::Flee,
                rank: 4,
                target: TargetKind::None,
                considerations: vec![
                    Consideration {
                        input: Input::Drive(DriveId::FEAR),
                        curve: Curve::Logistic { k: 10.0, x0: 0.45 }, // soft threshold ~0.45
                    },
                    Consideration {
                        // Local rally live → 0: a recruited crab standing in a marked sighting presses
                        // through gunfire to reach it, mirroring the berserk gate. Because this reads the
                        // LOCAL magnitude, a crab fleeing a firefight far from any beacon still flees (the
                        // old global-peak read suppressed flight mapwide). Fades below RALLY_MIN → fear resumes.
                        input: Input::Perc(Fact::RallyHere),
                        curve: Curve::Step {
                            threshold: RALLY_MIN,
                            below: 1.0,
                            above: 0.0,
                        },
                    },
                    Consideration {
                        // Local alarm live → 0: a crab whose neighbour was just shot retaliates instead of
                        // fleeing (the muster gate, twin of the berserk/rally gates). Local read, so only
                        // crabs within ~one room of the casualty press through fire; the rest of the swarm
                        // still flees a firefight. Fades below ALARM_MIN → fear resumes.
                        input: Input::Perc(Fact::AlarmHere),
                        curve: Curve::Step {
                            threshold: ALARM_MIN,
                            below: 1.0,
                            above: 0.0,
                        },
                    },
                ],
            },
        ],
    }
}

/// The scout brain — the swarm's recon arm (~20% of crabs). A tiny data-literal repertoire whose
/// roam→spot→recruit-via-stigmergy loop follows minimalist-agent foraging (Talamali, Bose, Haire, Xu,
/// Marshall & Reina, "Sophisticated collective foraging with minimalist agents: a swarm robotics test",
/// Swarm Intelligence 2019, DOI 10.1007/s11721-019-00176-9): **roam** far and fast hunting for prey; on a
/// sighting, **mark** it — track its live position and lay the vectorial rally pheromone toward it (Tang et
/// al. 2019, deposited by `crab::scout_mark_prey`) so the assault swarm converges; still **flee** gunfire.
/// Scouts don't latch/forage/carry — the 80% assault swarm (see `crab_brain`) does the killing. The
/// persistent scout/forager caste split is this game's design, NOT Talamali's model, which is homogeneous
/// (every agent both explores and carries, recruiting by mass/trail — no caste).
fn scout_brain() -> Brain {
    Brain {
        behaviors: vec![
            // Default: range the map hunting for prey. Unconditional low constant (like Wander) so a
            // roaming choice always exists when nothing else fires.
            Behavior {
                mode: Mode::Scout,
                rank: 0,
                target: TargetKind::None,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::SelfHealthFrac),
                    curve: Curve::Linear { m: 0.0, b: 0.15 },
                }],
            },
            // Mark the sighting: latches on `prey_spotted`, outranks roam so the scout stays with the prey
            // (approaching its tracked position) and keeps the rally pheromone fresh toward its live cell.
            Behavior {
                mode: Mode::Mark,
                rank: 2,
                target: TargetKind::TrackedPrey,
                considerations: vec![Consideration {
                    input: Input::Perc(Fact::PreySpotted),
                    curve: Curve::Step {
                        threshold: 0.5,
                        below: 0.0,
                        above: 1.0,
                    },
                }],
            },
            // Scouts panic too: flee down the THREAT gradient when afraid. No berserk gate (a scout's job
            // is recon, not a fearless press) — but the LOCAL alarm gate DOES apply: a scout standing in a
            // wounded neighbour's alarm bloom holds its ground (and keeps roaming/marking the fight) instead
            // of bolting, so "the crabs near a casualty go aggressive" covers the whole local swarm.
            Behavior {
                mode: Mode::Flee,
                rank: 3,
                target: TargetKind::None,
                considerations: vec![
                    Consideration {
                        input: Input::Drive(DriveId::FEAR),
                        curve: Curve::Logistic { k: 10.0, x0: 0.45 },
                    },
                    Consideration {
                        input: Input::Perc(Fact::AlarmHere),
                        curve: Curve::Step {
                            threshold: ALARM_MIN,
                            below: 1.0,
                            above: 0.0,
                        },
                    },
                ],
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    // Pure brain-shape checks — no App, no ECS (the seed-in/assert-out convention of `wfc.rs`).
    use super::*;
    use crate::ai::utility::validate_unconditional_default;

    #[test]
    fn creatures_ron_overlay_round_trips_and_replaces_by_name() {
        // FIX 4 (swarm promotion path): a `creatures.ron` entry deserializes and replaces exactly the named
        // creature, mirroring `roles.ron`. Serialize a REAL authored repertoire to RON, wrap it in the
        // creatures.ron map syntax, and read it back through the loader's exact map type — proving both that
        // `Behavior` round-trips and that `CreatureId` keys the override map (an evolved `elites_swarm.ron`
        // cell is transcribed into precisely this shape to promote it).
        let crab = authored_brains().crab.behaviors;
        let behaviors_ron = ron::to_string(&crab).expect("authored behaviours serialize to RON");
        let src = format!("{{ Crab: (behaviors: {behaviors_ron}) }}");
        let defs: std::collections::HashMap<CreatureId, CreatureDef> =
            ron::from_str(&src).expect("creatures.ron parses");
        assert_eq!(defs.len(), 1, "only the named creature is present");
        assert_eq!(
            defs[&CreatureId::Crab].behaviors.len(),
            crab.len(),
            "the crab repertoire round-trips intact"
        );
        // An unknown creature key is a LOUD parse error, never a silent skip (one path, no fallback).
        assert!(
            ron::from_str::<std::collections::HashMap<CreatureId, CreatureDef>>(
                "{ Nautilus: (behaviors: []) }"
            )
            .is_err(),
            "an unknown creature key must fail to parse, not be silently ignored"
        );
    }

    #[test]
    fn every_shipped_creature_brain_has_an_unconditional_default() {
        // `decide` screens out every behaviour scoring below MIN_SCORE. If a brain's whole repertoire is
        // gated on perception, nothing is eligible and `decide` falls through to behaviour 0 — a silent
        // wrong-action bug. Each creature brain must therefore keep a constant-score tail (Wander /
        // Forage). `init_brains` enforces this at startup; this test enforces it at compile-and-test time,
        // so a brain edit fails in CI rather than at launch.
        for (who, brain) in [
            ("smiley_brain", smiley_brain()),
            ("crab_brain", crab_brain()),
            ("scout_brain", scout_brain()),
        ] {
            validate_unconditional_default(&brain.behaviors, who)
                .unwrap_or_else(|e| panic!("{who} must ship an unconditional default: {e}"));
        }
    }
}
