//! **Noise as a resource you spend, not only a cost you leak** — the thrown lure (FVS-B-10 stage 1).
//!
//! Design: `docs/2026-08-01-acoustic-program.md`.
//!
//! # Why this exists
//!
//! The acoustic layer was machinery without a game attached. `NOISE_SWARM` propagates,
//! `crab_draw_to_din` scales the pull, `investigate_threshold` gates it and `Mode::Investigate` is
//! already in the brain — but nothing the player did ever *wrote* to that channel deliberately, so
//! squad noise was a pure tax. A tax is not a decision.
//!
//! Grimshaw & Schott (`10.26503/dl.v2007i1.313`) make the argument directly: one actor's sounds
//! "morph other players' soundscapes and so provide **new affordances**". Sound is a thing you use.
//! Throwing a lure pulls the swarm off the extraction route, and *that* is what makes the quiet half
//! a trade — noise discipline is only a choice if noise is sometimes worth spending.
//!
//! **No new `Mode`.** `Investigate` already means "go and look at that noise", which is exactly the
//! behaviour wanted; the lure just gives it something to answer. (Archive width is no longer a
//! constraint on this project, but reusing the mode is still the honest model.)
//!
//! # Habituation is the mechanic, not a detail
//!
//! A lure with no memory is a solved button: throw, walk past, repeat. So every lure a run places
//! makes the next one weaker, recovering over time ([`Habituation`]). That converts the verb from a
//! free pass into a resource with a *rhythm* — spend it when it matters, and let the swarm forget.
//!
//! # Determinism
//!
//! Gameplay, so `FixedUpdate` and pinned. Lures deposit into a stigmergy channel, and
//! `drain_deposits` applies each with a non-associative `f32 +=`, so the batch is emitted through a
//! stable total order — the same discipline `field.rs` documents for every other multi-source deposit
//! site. The key is `(position bits, throw seq)`: position alone is **not** total, because placement
//! snaps to a cell centre and two throws at one cell are bit-identical. This module's first draft
//! claimed it was, in a comment, and `sort_total!` caught it on the first test that threw twice at one
//! spot.

use bevy::prelude::*;

use crate::ai::field::{Deposit, FieldId, StigDeposits};

/// A thrown noisemaker, sitting on the floor and shouting into `NOISE_SWARM`.
#[derive(Component, Debug)]
pub struct Lure {
    /// Monotonic throw index — the stable tiebreak for [`tick_lures`]'s deposit ordering.
    ///
    /// Position is NOT unique: placement snaps to a floor **cell centre**, so two throws at the same
    /// cell are bit-identical. The first draft of this module claimed otherwise in a comment and
    /// `sort_total!` caught it on the first test that threw twice at one spot — the exact shape
    /// `CLAUDE.md` warns about ("don't trust a comment claiming a total order").
    pub seq: u64,
    /// Ticks of shouting left.
    pub ticks_left: u32,
    /// Per-tick deposit, already scaled by [`Habituation`] at throw time. Fixed for the lure's life:
    /// a lure that got quieter mid-flight because the player threw another one would be un-readable.
    pub amount: f32,
}

/// How bored the swarm is of being tricked, in `[0, 1]`. `0` = fully credulous.
///
/// Run-scoped state rather than per-lure: habituation is the *swarm's* memory, so a second lure
/// thrown across the map is still answering a swarm that has just been fooled. Recovers linearly, so
/// the verb comes back if the player stops leaning on it.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct Habituation(pub f32);

/// Monotonic throw counter. Never reset mid-run; reset with the rest of the lure state on run start.
#[derive(Resource, Default, Debug)]
pub struct LureSeq(pub u64);

/// Lures left this expedition. Reset from tuning on run start, exactly like `QuarantineSupply`.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct LureSupply(pub u32);

fn reset_lure_state(
    tuning: Res<crate::sim::SimTuning>,
    mut supply: ResMut<LureSupply>,
    mut hab: ResMut<Habituation>,
    mut seq: ResMut<LureSeq>,
) {
    *supply = LureSupply(tuning.lure.supply);
    *hab = Habituation(0.0);
    *seq = LureSeq(0);
}

/// Spawn a lure at `pos`, consuming a charge and deepening habituation.
///
/// `pub` so the click handler in `selection` and the Research Room palette can both place one through
/// **one** path — the supply spend, the habituation step and the amount scaling must not exist twice.
pub fn throw_lure(
    commands: &mut Commands,
    pos: Vec3,
    tuning: &crate::sim::LureTuning,
    supply: &mut LureSupply,
    hab: &mut Habituation,
    seq: &mut LureSeq,
) -> Option<Entity> {
    if supply.0 == 0 {
        return None;
    }
    supply.0 -= 1;
    // Scale BEFORE deepening, so the first lure of a run is at full strength rather than already
    // discounted — the player should get the honest version once.
    let amount = tuning.deposit * (1.0 - hab.0);
    hab.0 = (hab.0 + tuning.habituation_step).min(1.0);
    let id = seq.0;
    seq.0 += 1;
    Some(
        commands
            .spawn((
                Lure { seq: id, ticks_left: tuning.duration_ticks, amount },
                Transform::from_translation(pos),
                crate::session::run_scoped(),
            ))
            .id(),
    )
}

/// Shout into `NOISE_SWARM`, then expire. Habituation recovers here too, so it decays on the same
/// clock the lures live on.
fn tick_lures(
    mut commands: Commands,
    tuning: Res<crate::sim::SimTuning>,
    mut hab: ResMut<Habituation>,
    mut deposits: ResMut<StigDeposits>,
    mut lures: Query<(Entity, &Transform, &mut Lure)>,
) {
    hab.0 = (hab.0 - tuning.lure.habituation_recovery).max(0.0);

    // `drain_deposits` sums with a non-associative `f32 +=`, so three lures whose radii overlap would
    // smear the channel differently depending on ECS query order. Emit in a stable total order.
    //
    // Position alone is NOT total — placement snaps to a cell centre, so two throws at one cell are
    // bit-identical. `Lure::seq` is the tiebreak: monotonic per throw, never reused within a run.
    // Position leads so the ordering stays spatially coherent; `seq` only decides genuine ties.
    let mut batch: Vec<(u32, u32, u32, u64, Entity)> = lures
        .iter()
        .map(|(e, tf, l)| {
            (
                tf.translation.x.to_bits(),
                tf.translation.y.to_bits(),
                tf.translation.z.to_bits(),
                l.seq,
                e,
            )
        })
        .collect();
    crate::sort_total!(&mut batch, |k: &(u32, u32, u32, u64, Entity)| (k.0, k.1, k.2, k.3));

    for (.., e) in batch {
        let Ok((_, tf, mut lure)) = lures.get_mut(e) else { continue };
        deposits.0.push(Deposit {
            pos: tf.translation,
            field: FieldId::NOISE_SWARM,
            amount: lure.amount,
        });
        lure.ticks_left = lure.ticks_left.saturating_sub(1);
        if lure.ticks_left == 0 {
            commands.entity(e).despawn();
        }
    }
}

pub struct LurePlugin;

impl Plugin for LurePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LureSupply>()
            .init_resource::<Habituation>()
            .init_resource::<LureSeq>()
            .add_systems(
                OnEnter(crate::session::RunState::Active),
                reset_lure_state.in_set(crate::session::RunBuild::Config),
            )
            // BEFORE `AiSet::Deposits` drains the queue, so a lure thrown this tick is audible this
            // tick — the same ordering `crab_alarm_on_damage` uses for the ALARM bloom. A verb whose
            // effect lands a tick late reads as unresponsive.
            .add_systems(
                FixedUpdate,
                tick_lures
                    .before(crate::ai::AiSet::Deposits)
                    .run_if(in_state(crate::session::RunState::Active)),
            );
    }
}
