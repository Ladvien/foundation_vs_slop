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
    decide, Behavior, Consideration, Curve, Fact, Input, Mode, Perception, TargetKind,
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
    /// Attached to crabs in Phase 4.
    #[allow(dead_code)]
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
}

/// Recompute the field hotspots once per frame (runs in `AiSet::FieldUpdate`). RALLY has no global peak
/// — it is a vectorial pheromone read locally per-crab (see [`crate::ai::field::RallyField`]).
pub fn update_hotspots(mut hot: ResMut<FieldHotspots>, stig: Option<Res<Stig>>, dungeon: Res<Dungeon>) {
    if let Some(stig) = stig {
        hot.scent = stig.hotspot(FieldId::SCENT, &dungeon);
        hot.meat = stig.hotspot(FieldId::MEAT, &dungeon);
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
    alarm: Res<crate::nest::NestAlarm>,
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

        let pos = tf.translation;
        let mut nearest_dist = f32::MAX;
        let mut nearest_unit = None;
        // Crabs scan all prey (units + boss); the boss scans only units (never itself).
        let mut scan = |t: &Transform| {
            let d = (t.translation.xz() - pos.xz()).length();
            if d < nearest_dist {
                nearest_dist = d;
                nearest_unit = Some(t.translation);
            }
        };
        match brain_id {
            // Crabs and scouts both scan all prey (units + boss); the boss scans only units.
            BrainId::Crab | BrainId::Scout => {
                for t in &prey {
                    scan(t);
                }
            }
            BrainId::Smiley => {
                for t in &units {
                    scan(t);
                }
            }
        }

        let perc = Perception {
            pos,
            nearest_unit,
            nearest_dist: if nearest_unit.is_some() {
                nearest_dist
            } else {
                999.0
            },
            health_frac: if health.max > 0.0 {
                (health.current / health.max).clamp(0.0, 1.0)
            } else {
                0.0
            },
            drives: drives.v,
            scent_hotspot: hotspots.scent.0,
            scent_val: hotspots.scent.1,
            threat_here: stig.sample(FieldId::THREAT, &dungeon, pos),
            meat_hotspot: hotspots.meat.0,
            meat_val: hotspots.meat.1,
            carrying: if carry.is_some_and(|c| c.hauling) {
                1.0
            } else {
                0.0
            },
            berserk: if alarm.0 > 0.0 { 1.0 } else { 0.0 },
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
            // The Carry destination (the nest) is resolved per-crab by `carry_gibs` from the lifted
            // gib, not from the global hotspot — decide() only picks the mode here.
            TargetKind::Nest => None,
            // A marking scout aims at the live prey it is tracking (to keep laying the rally pheromone
            // toward its current position). Rally movement itself reads the local vector in `crab_rally`,
            // so the Rally behaviour needs no aim point here.
            TargetKind::TrackedPrey => scout.and_then(|s| s.tracked_prey()),
        };
    }
}

/// Insert the brain registry (the developer's behaviour catalogue).
pub fn init_brains(mut commands: Commands) {
    commands.insert_resource(AiBrains {
        smiley: smiley_brain(),
        crab: crab_brain(),
        scout: scout_brain(),
    });
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
/// A live rally *also* gates Flee off (like the nest-berserk gate), so a recruited swarm presses through
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
            // Panic: flee down the THREAT gradient when afraid — outranks everything (drops the load).
            // Suppressed while berserk (a nest under attack): the swarm ignores fear and presses the squad.
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
                        // Berserk → 0, calm → 1: a nest under attack turns off flight entirely.
                        input: Input::Perc(Fact::Berserk),
                        curve: Curve::Step {
                            threshold: 0.5,
                            below: 1.0,
                            above: 0.0,
                        },
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

/// The scout brain — the swarm's recon arm (~20% of crabs). A tiny data-literal repertoire that models
/// ant scout-recruitment foraging by minimalist agents (Talamali, Bose, Haire, Xu, Marshall & Reina,
/// "Sophisticated collective foraging with minimalist agents: a swarm robotics test", Swarm Intelligence
/// 2019, DOI 10.1007/s11721-019-00176-9): **roam** far and fast hunting for prey; on a sighting, **mark** it —
/// track its live position and lay the vectorial rally pheromone toward it (Tang et al. 2019, deposited by
/// `crab::scout_mark_prey`) so the assault swarm converges; still **flee** gunfire. Scouts don't
/// latch/forage/carry — the 80% assault swarm (see `crab_brain`) does the killing.
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
