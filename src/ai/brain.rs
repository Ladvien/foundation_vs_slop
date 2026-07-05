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
use crate::util::hash01;

/// A blood frenzy must reach this SCENT strength before the boss paths to it (matches the HuntBlood gate).
const HUNT_SCENT_MIN: f32 = 1.0;

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
}

/// Which brain an agent uses.
#[derive(Component, Clone, Copy)]
pub enum BrainId {
    Smiley,
    /// Attached to crabs in Phase 4.
    #[allow(dead_code)]
    Crab,
}

/// The current decision, written by `think`, read by the locomotion systems each frame.
#[derive(Component)]
pub struct ActiveBehavior {
    pub mode: Mode,
    pub target: Option<Vec3>,
    pub rng: u32,
}

impl ActiveBehavior {
    /// Seed the per-agent decision RNG + stagger the first think from a stable per-spawn hash.
    pub fn new(pos: Vec3) -> Self {
        Self {
            mode: Mode::Wander,
            target: None,
            rng: (hash01(pos) * 4_000_000.0) as u32 | 1,
        }
    }
}

/// Per-agent decision throttle (counts down to the next think).
#[derive(Component)]
pub struct ThinkTimer(pub f32);

impl ThinkTimer {
    pub fn staggered(pos: Vec3) -> Self {
        ThinkTimer(hash01(pos + Vec3::splat(3.0)) * THINK_INTERVAL)
    }
}

/// The peak of a field each frame — a shared, computed-once signal (Mark Ch.30) so agents read a
/// global "where's the frenzy" without each scanning the grid.
#[derive(Resource, Default)]
pub struct FieldHotspots {
    pub scent: (Vec3, f32),
    pub meat: (Vec3, f32),
}

/// Recompute the field hotspots once per frame (runs in `AiSet::FieldUpdate`).
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
    dungeon: Res<Dungeon>,
    hotspots: Res<FieldHotspots>,
    brains: Res<AiBrains>,
    units: Query<&Transform, With<Unit>>,
    mut agents: Query<
        (
            &Transform,
            &BrainId,
            &Drives,
            &Health,
            &mut ActiveBehavior,
            &mut ThinkTimer,
            Option<&crate::crab::CrabCarry>,
        ),
        Without<Unit>,
    >,
) {
    let dt = time.delta_secs();
    for (tf, brain_id, drives, health, mut active, mut timer, carry) in &mut agents {
        timer.0 -= dt;
        if timer.0 > 0.0 {
            continue;
        }
        timer.0 = THINK_INTERVAL;

        let pos = tf.translation;
        let mut nearest_dist = f32::MAX;
        let mut nearest_unit = None;
        for u in &units {
            let d = (u.translation.xz() - pos.xz()).length();
            if d < nearest_dist {
                nearest_dist = d;
                nearest_unit = Some(u.translation);
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
        };

        let brain = match brain_id {
            BrainId::Smiley => &brains.smiley,
            BrainId::Crab => &brains.crab,
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
        };
    }
}

/// Insert the brain registry (the developer's behaviour catalogue).
pub fn init_brains(mut commands: Commands) {
    commands.insert_resource(AiBrains {
        smiley: smiley_brain(),
        crab: crab_brain(),
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
/// Flee(2) > Latch(1) > Forage(0). A pursuing crab latches onto a unit and feeds; but when the shared
/// THREAT field spikes (gunfire), its FEAR climbs and the rank-2 Flee outranks everything, so the swarm
/// scatters — then FEAR decays as THREAT evaporates and foraging resumes. Nobody scripted "scatter".
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
            // Climb onto and feed on a unit once close AND hungry.
            Behavior {
                mode: Mode::Latch,
                rank: 1,
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
                rank: 1,
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
                rank: 2,
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
            // Panic: flee down the THREAT gradient when afraid — outranks everything (drops the load).
            Behavior {
                mode: Mode::Flee,
                rank: 3,
                target: TargetKind::None,
                considerations: vec![Consideration {
                    input: Input::Drive(DriveId::FEAR),
                    curve: Curve::Logistic { k: 10.0, x0: 0.45 }, // soft threshold ~0.45
                }],
            },
        ],
    }
}
