//! What an SCP-1048 attack *does*: dread into the stigmergy fields, damage into `Health`, and the
//! ear-growth affliction SCP-1048-A's shriek leaves behind.
//!
//! Kept apart from [`super::behavior`], which owns only motion and pose. Every system here writes into
//! something **shared** — a field grid, another entity's health, a unit's affliction — so each one
//! needs its own determinism argument, and those arguments are the reason this is a separate file.
//!
//! ## The ear growths (SCP-1048-A)
//!
//! Canon: the shriek blinds and deafens everyone within ~10 m, and within ~5 m ear-like growths spread
//! over the victim, suffocating them within about three minutes. That is modelled as **damage that
//! accumulates under exposure, is repaired at a constant rate once exposure ends, and kills only above
//! a threshold** — the General Unified Threshold model of Survival (Jager, Albert, Preuss & Ashauer,
//! "General Unified Threshold Model of Survival — a Toxicokinetic-Toxicodynamic Framework for
//! Ecotoxicology", Environ. Sci. Technol. 2011, doi:10.1021/es103092a). `growth_rate` is the
//! accumulation term, `growth_decay` the repair term, `asphyxiate_threshold` the threshold. The same
//! damage/repair/threshold shape has since been carried to non-chemical stressors (Mangold-Döring et
//! al., Environ. Sci. Technol. 2023, doi:10.1021/acs.est.3c05079), which is the licence for using it
//! for an anomalous one.
//!
//! Setting `growth_decay` to 0 gives an *incurable* world. That is a real world the search may reach,
//! deliberately reachable — not a degenerate one — which is why its genome bound is floored at zero.

use bevy::prelude::*;

use super::{AnimState, Scp1048, Scp1048Seed, Scp1048State, Scp1048Variant};
use crate::ai::field::{Deposit, FieldId, StigDeposits};
use crate::health::Health;
use crate::sim::SimTuning;
use crate::squad::{SquadMember, Unit};

/// How far along the ear-growth affliction a squad unit is, in `[0, 1]`.
///
/// **Always present on every unit**, inserted at `squad::spawn_unit` and never added or removed at
/// runtime — the `parasite::Infestation` idiom. A component that appeared when a unit was first
/// screamed at would migrate its archetype mid-run and make ECS iteration order run-dependent.
///
/// Advanced on `FixedUpdate` but deliberately **not** folded into `snapshot_hash` (which folds only
/// `Transform` and `Health`), exactly like `Infestation::timer`. It reaches the hash through the
/// damage it eventually causes, which is the thing worth pinning.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct EarGrowth {
    pub severity: f32,
}

/// Is this bear mid-shriek? The exposure window is the whole `scream` clip, not a single tick — the
/// clip is authored as a sustained held shiver, and a victim who runs during it should take less.
fn is_screaming(bear: &Scp1048, state: &Scp1048State) -> bool {
    bear.variant == Scp1048Variant::EarCopy && state.anim == AnimState::Attack
}

/// Dread into the shared `THREAT_ANOMALY` channel — the same one the watcher and a roused SCP-150
/// brood emit, and the only creature channel units read through walls (via the Psionic).
///
/// Two emitters, both gated so a bear is silent unless it is actually menacing someone:
/// - a **raging copy** radiates a standing dread at `rage_dread_rate` (a wandering one emits nothing);
/// - **SCP-1048-A's scream** stamps a one-shot burst at `scream_dread` over `pain_radius`, and also
///   into `NOISE_SWARM` so the shriek is audible as well as dreadful.
///
/// The benign original never emits: it is not hostile, and a teddy bear that terrified the squad from
/// across the map would destroy the "keep eyes on it" counterplay the whole module is built around.
///
/// **Determinism.** Overlapping deposit discs accumulate into the grid with non-associative float
/// `+=`, so positions are value-sorted before emitting — the same reason `enemy::deposit_anomaly_aura`
/// and `parasite::deposit_manca_dread` sort. The two lists are sorted and emitted **separately**
/// because they carry different amounts: a single merged list keyed on position alone would be a key
/// that is a *prefix of the value*, which is exactly the tie-breaking trap the determinism rules name.
///
/// **Ordering, and the one-tick lag it implies.** The AI phase runs Deposits → FieldUpdate → Drives →
/// Think, so this is ordered `.before(AiSet::Deposits)` — which puts it *before* the executor that
/// writes `strike_landed`, not after. It therefore reads the previous tick's bear state. That is
/// deliberate and matches `parasite::deposit_manca_dread`: the dread is evaporated and diffused on the
/// same pass it arrives, and one fixed tick of lag on a fear field is imperceptible. It is also why
/// `strike_landed` is cleared at the *top* of the executor rather than by this system — the flag must
/// survive until the next tick's deposit pass has read it.
pub(crate) fn deposit_bear_dread(
    time: Res<Time>,
    sim: Res<SimTuning>,
    bears: Query<(&Scp1048, &Scp1048State, &Transform)>,
    mut deposits: ResMut<StigDeposits>,
) {
    let t = &sim.scp1048;

    // ── The standing dread of a raging copy ──
    let rage_amount = t.rage_dread_rate * time.delta_secs();
    let mut raging: Vec<Vec3> = bears
        .iter()
        .filter(|(bear, state, _)| bear.variant.is_hostile() && state.anim == AnimState::Rage)
        .map(|(_, _, tf)| tf.translation)
        .collect();
    // SORT-OK: bare positions — whole value, ties are identical deposits (interchangeable).
    raging.sort_unstable_by_key(|p| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));
    for pos in raging {
        deposits.0.push(Deposit { pos, field: FieldId::THREAT_ANOMALY, amount: rage_amount });
    }

    // ── The shriek ──
    let mut screams: Vec<Vec3> = bears
        .iter()
        .filter(|(bear, state, _)| {
            bear.variant == Scp1048Variant::EarCopy && state.strike_landed
        })
        .map(|(_, _, tf)| tf.translation)
        .collect();
    // SORT-OK: bare positions — whole value, ties are identical deposits (interchangeable).
    screams.sort_unstable_by_key(|p| (p.x.to_bits(), p.y.to_bits(), p.z.to_bits()));
    for pos in screams {
        deposits.0.push(Deposit { pos, field: FieldId::THREAT_ANOMALY, amount: t.scream_dread });
        deposits.0.push(Deposit { pos, field: FieldId::NOISE_SWARM, amount: t.scream_dread });
    }
}

/// Grow — or heal — ear tissue on every squad unit, once per tick.
///
/// **Determinism: this needs no sort, and the reason is narrower than it first looks.**
///
/// Each unit is written **exactly once per tick**, with a single delta chosen by a boolean: is it
/// inside the growth band of *any* screamer? `any()` is order-independent (it is a pure predicate over
/// a set, and the same set either contains a qualifying screamer or does not), and one write cannot be
/// reordered with itself. That — not any commutation law — is the whole argument.
///
/// It is worth being precise, because the obvious-looking justification is **false**: clamped f32
/// accumulation does *not* commute, since float addition is not associative. Concretely,
/// `(0.13 + 0.4) + 0.02 == 0.54999995` while `(0.13 + 0.02) + 0.4 == 0.55`. So a version of this
/// system that stacked a dose **per screamer** would be order-dependent and would need a
/// `sort_total!` over the contributions, exactly like `scp1048_strike_damage` below.
/// `once_per_unit_is_what_makes_this_order_independent` pins the distinction.
///
/// **The once-per-unit shape is therefore load-bearing, not an optimisation.**
///
/// Note the asymmetry between the bands: `pain_radius` (the dread above) reaches further than
/// `growth_radius` (here). The shriek terrifies a wider circle than it maims, which is what makes
/// backing out of the inner band a real, survivable decision rather than a coin flip.
pub(crate) fn scp1048_scream_exposure(
    time: Res<Time>,
    sim: Res<SimTuning>,
    bears: Query<(&Scp1048, &Scp1048State, &Transform)>,
    mut units: Query<(&Transform, &mut EarGrowth), With<Unit>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let t = &sim.scp1048;
    let screamers: Vec<Vec3> = bears
        .iter()
        .filter(|(bear, state, _)| is_screaming(bear, state))
        .map(|(_, _, tf)| tf.translation)
        .collect();
    let r_sq = t.growth_radius * t.growth_radius;

    for (tf, mut growth) in &mut units {
        let exposed = screamers
            .iter()
            .any(|s| (s.xz() - tf.translation.xz()).length_squared() <= r_sq);
        growth.severity = if exposed {
            (growth.severity + t.growth_rate * dt).min(1.0)
        } else {
            (growth.severity - t.growth_decay * dt).max(0.0)
        };
    }
}

/// Once the growths have covered a unit, they suffocate it.
///
/// **Determinism.** Per-unit, reading only that unit's own severity — no shared state, no query-order
/// decision. But it MUST be ordered inside `health::HealthDamage` and explicitly after the previous
/// link: several writers can hit one unit's `Health` on the same tick, and float addition is not
/// associative, so the chain's order is part of the result. That is the M1 lesson recorded in
/// `tests/replay.rs`'s re-pin history, and this is the eighth link in that chain.
pub(crate) fn scp1048_asphyxiate(
    time: Res<Time>,
    sim: Res<SimTuning>,
    mut units: Query<(&EarGrowth, &mut Health), With<Unit>>,
) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    let t = &sim.scp1048;
    for (growth, mut health) in &mut units {
        if growth.severity >= t.asphyxiate_threshold {
            health.apply_damage(t.asphyxiate_dps * dt);
        }
    }
}

/// Land the blows that connected this tick: SCP-1048-B's tantrum and SCP-1048-C's gun and club.
///
/// SCP-1048-A is deliberately absent — its shriek deals **no** direct damage. What it does is start
/// the ear growths, which kill on their own schedule. That is the whole point of the variant: the copy
/// that never touches you is the one that kills you slowest and most certainly.
///
/// **Determinism.** Damage is additive into a shared `Health`, so when several bears strike one unit
/// the summation order is part of the result. Contributions are therefore gathered per unit and summed
/// in a **total** order — `Scp1048Seed`, which is unique per bear and never position-derived (a copy
/// spawns in its parent's own cell, so a position key could not break a position tie). Units are keyed
/// by `SquadMember`, which is likewise unique and stable, never `Entity` (ids are recycled and are not
/// reproducible across runs).
pub(crate) fn scp1048_strike_damage(
    sim: Res<SimTuning>,
    bears: Query<(&Scp1048, &Scp1048State, &Scp1048Seed, &Transform)>,
    clock: Res<crate::session::RunClock>,
    mut units: Query<
        (&Transform, &SquadMember, &mut Health, Option<&mut crate::knowledge::Knowledge>),
        With<Unit>,
    >,
) {
    let t = &sim.scp1048;
    if t.strike_damage <= 0.0 {
        return;
    }
    // Every blow that connected this tick, with the seed that will order it.
    let strikers: Vec<(u32, Vec3)> = bears
        .iter()
        .filter(|(bear, state, _, _)| {
            state.strike_landed
                && matches!(bear.variant, Scp1048Variant::InfantArm | Scp1048Variant::Scrap)
        })
        .map(|(_, _, seed, tf)| (seed.0, tf.translation))
        .collect();
    if strikers.is_empty() {
        return;
    }
    let reach_sq = t.strike_range * t.strike_range;

    for (tf, _member, mut health, mut knowledge) in &mut units {
        let mut hits: Vec<u32> = strikers
            .iter()
            .filter(|(_, bpos)| (bpos.xz() - tf.translation.xz()).length_squared() <= reach_sq)
            .map(|(seed, _)| *seed)
            .collect();
        if hits.is_empty() {
            continue;
        }
        // SORT-OK: `Scp1048Seed` is unique per bear, so this is a total order over the contributions;
        // the damage amount is the same for every striker, so the seed IS the whole distinguishing
        // value here, not a prefix of it.
        hits.sort_unstable();
        for _ in &hits {
            health.apply_damage(t.strike_damage);
        }
        // FVS-O-1b — firsthand acquisition. Being struck is how an operative learns that 1048's copies
        // are lethal, and it is deliberately scoped to the one who was hit: a bystander watching it
        // happen would acquire a `Witnessed` belief, which is FVS-O-3's job, not this system's.
        //
        // `Option<&mut Knowledge>` because a bare-`App` unit test may spawn a unit without one; every
        // operative `spawn_unit` builds carries it. Writing only this entity's own component keeps the
        // system order-independent, so it still needs no canonical sort.
        if let Some(k) = knowledge.as_mut() {
            crate::knowledge::coupling::learn_from_a_blow(
                k,
                crate::knowledge::Subject::BearCopies,
                clock.ticks,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The GUTS accumulate/repair step, mirroring `scp1048_scream_exposure`'s single per-unit write.
    fn step(severity: f32, delta: f32) -> f32 {
        (severity + delta).clamp(0.0, 1.0)
    }

    #[test]
    fn once_per_unit_is_what_makes_this_order_independent() {
        // The determinism claim is "one write per unit per tick", NOT "clamped deltas commute" — and
        // this pins the difference, because the second statement is FALSE in f32. Float addition is
        // not associative, so a version of this system that stacked a dose per screamer would be
        // order-dependent and would need a total sort over the contributions.
        //
        // If someone "optimises" the exposure pass into a per-screamer accumulate, this test is the
        // record of why that is a determinism break rather than a refactor.
        let (s, a, b) = (0.13f32, 0.4f32, 0.02f32);
        assert_ne!(
            step(step(s, a), b),
            step(step(s, b), a),
            "if clamped f32 deltas ever DO commute, this comment and the sort rule need revisiting"
        );

        // What the system actually relies on: the exposure predicate is a pure `any()` over the
        // screamer set, so shuffling that set cannot change the single delta a unit receives.
        let unit = Vec2::new(3.0, 4.0);
        let screamers = [Vec2::new(3.2, 4.1), Vec2::new(40.0, 40.0), Vec2::new(2.5, 3.6)];
        let r_sq = 5.0f32 * 5.0;
        let exposed = |order: [usize; 3]| {
            order.iter().any(|&i| (screamers[i] - unit).length_squared() <= r_sq)
        };
        for order in [[0, 1, 2], [2, 1, 0], [1, 0, 2], [1, 2, 0], [0, 2, 1], [2, 0, 1]] {
            assert_eq!(exposed(order), exposed([0, 1, 2]), "exposure must not depend on order");
        }
    }

    #[test]
    fn severity_saturates_and_never_goes_negative() {
        assert_eq!(step(0.99, 0.5), 1.0, "severity must saturate at fully covered");
        assert_eq!(step(0.01, -0.5), 0.0, "healing must not drive severity negative");
    }

    #[test]
    fn the_shipped_dose_matches_the_canon_timings() {
        // Canon: growths cover the body in ~20 s of exposure, and death follows within ~3 minutes.
        // These are the numbers the shipped defaults are chosen to hit, so a re-tune that quietly
        // breaks the article's pacing shows up here.
        let t = SimTuning::default().scp1048;
        let secs_to_full = 1.0 / t.growth_rate;
        assert!((15.0..=30.0).contains(&secs_to_full), "~20 s to full cover, got {secs_to_full}");
        let unit_hp = SimTuning::default().combat.unit_hp;
        let secs_to_kill = unit_hp / t.asphyxiate_dps;
        assert!((120.0..=240.0).contains(&secs_to_kill), "~3 min to asphyxiate, got {secs_to_kill}");
    }

    #[test]
    fn the_lethal_band_is_wide_enough_to_outlive_the_scream() {
        // **The regression this test exists for.** With `asphyxiate_threshold == 1.0` the lethal band
        // is the single point at the top of the severity range, so the moment the shriek stops
        // `growth_decay` drops the victim under it on the very next tick — suffocation could then only
        // ever tick while the bear was still actively screaming, and canon's "every person afflicted
        // died within 3 minutes" would be unreachable. The band must be a band.
        let t = SimTuning::default().scp1048;
        assert!(
            t.asphyxiate_threshold < 1.0,
            "asphyxiate_threshold must leave headroom above it, got {}",
            t.asphyxiate_threshold
        );
        // How long a victim taken to full cover keeps suffocating after being pulled clear.
        let dying_secs = (1.0 - t.asphyxiate_threshold) / t.growth_decay.max(f32::EPSILON);
        assert!(
            dying_secs >= 20.0,
            "a rescued victim should keep suffocating for a while, got {dying_secs:.1}s"
        );
        // ...but still be savable, which is the counterplay half of the design.
        let unit_hp = SimTuning::default().combat.unit_hp;
        assert!(
            dying_secs * t.asphyxiate_dps < unit_hp,
            "pulling someone out of the radius must be able to save them at the shipped tuning"
        );
    }

    #[test]
    fn the_dread_band_reaches_further_than_the_lethal_band() {
        // The shriek must terrify a wider circle than it maims, or backing out of the growth radius
        // stops being a decision the player can make.
        let t = SimTuning::default().scp1048;
        assert!(
            t.pain_radius > t.growth_radius,
            "pain {} must out-reach growths {}",
            t.pain_radius,
            t.growth_radius
        );
    }

    #[test]
    fn only_the_ear_bear_screams() {
        let mut state = Scp1048State::new();
        state.anim = AnimState::Attack;
        assert!(is_screaming(&Scp1048 { variant: Scp1048Variant::EarCopy }, &state));
        for v in [Scp1048Variant::Original, Scp1048Variant::InfantArm, Scp1048Variant::Scrap] {
            assert!(!is_screaming(&Scp1048 { variant: v }, &state), "{v:?} must not scream");
        }
    }
}
