//! Co-evolution tests — moved wholesale out of the former single-file `coevolve.rs`.

use super::*;
use crate::squad_ai::surprise::{ActorKind, Context, Decision, FearBucket};

fn decision(actor: ActorKind, mode: Mode) -> Decision {
    Decision {
        actor_id: 0,
        context: Context {
            actor,
            fear: FearBucket::Calm,
            threat_known: false,
            ally_down: false,
            past_leash: false,
        },
        mode,
        witnessed: true,
    }
}

#[test]
fn the_authored_pairing_is_feasible() {
    // The search must admit the shipped brains, or it is measuring the wrong space.
    let t = Templates::authored();
    let squad = SquadGenome::authored(&t);
    let swarm = SwarmGenome::authored(&t);
    assert!(feasible(&t, &squad, &swarm).is_ok(), "{:?}", feasible(&t, &squad, &swarm));
    assert!(brains_of(&t, &squad, &swarm).is_ok());
}

#[test]
fn brains_of_rejects_a_mis_shaped_squad_genome() {
    let t = Templates::authored();
    let mut squad = SquadGenome::authored(&t);
    squad.0.pop();
    let swarm = SwarmGenome::authored(&t);
    assert!(brains_of(&t, &squad, &swarm).is_err(), "a short genome must never silently decode");
}

#[test]
fn descriptors_read_the_axes_a_player_perceives() {
    let trace = EpisodeTrace {
        decisions: vec![
            decision(ActorKind::Role(RoleId::Gunman), Mode::Engage),
            decision(ActorKind::Role(RoleId::Gunman), Mode::Overwatch),
            decision(ActorKind::Role(RoleId::Medic), Mode::TendWounded),
            decision(ActorKind::Role(RoleId::Medic), Mode::Wander),
            decision(ActorKind::Crab, Mode::Latch),
            decision(ActorKind::Crab, Mode::Flee),
            decision(ActorKind::Scout, Mode::Rally),
            decision(ActorKind::Smiley, Mode::Chase),
        ],
    };
    let outcome = EpisodeOutcome { cells_covered: 25, reachable_cells: 100, ..Default::default() };

    let squad = squad_descriptor(&trace, &outcome);
    assert!((squad.aggression - 0.5).abs() < 1e-6, "2 of 4 unit decisions press the fight");
    assert!((squad.exploration - 0.25).abs() < 1e-6);

    let swarm = swarm_descriptor(&trace);
    assert!((swarm.aggression - 0.75).abs() < 1e-6, "Latch/Rally/Chase of 4 creature decisions");
    assert!((swarm.exploration - 0.75).abs() < 1e-6, "1 of 4 fled");
}

#[test]
fn descriptors_of_an_empty_trace_are_zero_not_nan() {
    let empty = EpisodeTrace::default();
    let outcome = EpisodeOutcome::default();
    let s = squad_descriptor(&empty, &outcome);
    assert_eq!((s.aggression, s.exploration), (0.0, 0.0));
    // A swarm that never decided has zero aggression — and `persistence` reads 1.0, since nothing fled.
    let w = swarm_descriptor(&empty);
    assert_eq!(w.aggression, 0.0);
    assert!(w.exploration.is_finite());
}

#[test]
fn population_handles_stay_valid_when_an_insert_is_rejected() {
    // Handles index `store` and are never recycled, so a rejected insert must not invalidate an
    // incumbent's handle.
    let mut pop: Population<u32> = Population::new(4);
    let d = BehaviorDescriptor::new(0.5, 0.5);
    assert!(pop.insert(d, 0.8, 111));
    assert!(!pop.insert(d, 0.2, 222), "worse fitness must be rejected");
    let elite = pop.archive.best().expect("an elite");
    assert_eq!(pop.get(elite.genome), Some(&111), "the incumbent's handle still resolves");
}

#[test]
fn reeval_insert_resolves_a_contested_cell_by_the_common_opponent_score() {
    // The Phase-5 elitism logic (no rollouts — the re-eval is a closure). An empty cell takes the
    // challenger without consulting the incumbent; a contest is decided by re-scoring the incumbent on
    // the challenger's conditions, and the incumbent's stored fitness is refreshed to that fresh value.
    let mut pop: Population<u32> = Population::new(4);
    let d = BehaviorDescriptor::new(0.5, 0.5);

    assert!(pop
        .try_insert_with_reeval(d, 0.8, 111, |_| panic!("no re-eval on an empty cell"))
        .unwrap());

    // Incumbent re-scores >= challenger under the common opponents → it holds, refreshed to 0.95.
    assert!(!pop
        .try_insert_with_reeval(d, 0.9, 222, |&g| {
            assert_eq!(g, 111, "the incumbent genome is re-evaluated");
            Ok(Some(0.95))
        })
        .unwrap());
    let inc = pop.archive.incumbent(d).expect("held");
    assert_eq!(pop.get(inc.genome), Some(&111));
    assert!((inc.fitness - 0.95).abs() < 1e-6, "fitness refreshed to the common-opponent score");

    // Incumbent re-scores lower → challenger wins.
    assert!(pop.try_insert_with_reeval(d, 0.5, 333, |_| Ok(Some(0.1))).unwrap());
    assert_eq!(pop.get(pop.archive.incumbent(d).unwrap().genome), Some(&333));

    // Incumbent inadmissible (produces no encounter) under the challenger's conditions → challenger wins.
    let d2 = BehaviorDescriptor::new(0.1, 0.1);
    assert!(pop.try_insert_with_reeval(d2, 0.4, 444, |_| panic!("empty")).unwrap());
    assert!(pop.try_insert_with_reeval(d2, 0.2, 555, |_| Ok(None)).unwrap());
    assert_eq!(pop.get(pop.archive.incumbent(d2).unwrap().genome), Some(&555));
}

#[test]
fn sampling_is_deterministic_under_a_seed_and_empty_archives_yield_nothing() {
    let mut pop: Population<u32> = Population::new(4);
    assert!(pop.sample_parent(&mut seeded(1)).is_none(), "an empty archive has no parent");
    assert!(pop.sample_opponents(3, &mut seeded(1)).is_empty());

    pop.insert(BehaviorDescriptor::new(0.1, 0.1), 0.5, 7);
    pop.insert(BehaviorDescriptor::new(0.9, 0.9), 0.5, 9);
    let a: Vec<u32> = pop.sample_opponents(5, &mut seeded(42)).into_iter().copied().collect();
    let b: Vec<u32> = pop.sample_opponents(5, &mut seeded(42)).into_iter().copied().collect();
    assert_eq!(a, b, "opponent sampling must be reproducible from the seed");
    assert_eq!(a.len(), 5, "sampling is with replacement, so a sparse archive still yields k");
}

#[test]
fn mean_is_order_independent() {
    // Float addition is not associative; the search's reproducibility depends on this.
    let xs = [0.1f32, 0.2, 0.3, 0.7, 0.9];
    let mut ys = xs;
    ys.reverse();
    assert_eq!(mean(&xs), mean(&ys));
}

/// **A canary on `SIGMA`** — and a regression test for a bug this test found: with the slope sign left
/// free, `Linear{m}` went negative half the time, `guaranteed_floor` lost both unconditional tails
/// (`wander`, `follow_anchor`, both authored `m = 0.0`), and **0 of 32** children were feasible — the
/// search would have spun forever. `ParamKind::SignLocked` fixed it.
///
/// # Why this was recalibrated (2026-08-05)
///
/// It asserted `>= 16` joint-feasible children out of **64 draws**, and it was red at `13/64`. That was
/// not a regression. Measured over 2000 draws, the joint single-draw rate is **0.255** — so the old bar
/// sat exactly *at* the true mean, where the count's standard deviation is
/// `sqrt(64 · 0.255 · 0.745) ≈ 3.5`. A threshold at the mean fails for about half of all seeds by
/// construction, and `13` is 0.9σ low: an ordinary draw, not a signal. It went red when something
/// perturbed the RNG stream (the `coevolve.rs` file split, `72e3423`), which is the only reason a
/// *seeded* coin flip changes its answer.
///
/// Two things replace it, both measured rather than assumed:
///
/// * **The per-side rates, with enough samples to mean something.** They are wildly asymmetric, which
///   the old joint number hid and the old comment got wrong: `squad` is **0.902** and `swarm` is
///   **0.281**, so the swarm side is the entire constraint. At `n = 2000` each rate has σ ≈ 0.01, so
///   the floors below are ~10σ away and cannot flap.
/// * **What production actually needs.** `propose_squad` and `propose_swarm` redraw *independently*,
///   `MAX_MUTATION_ATTEMPTS` each — they never require a jointly-feasible single draw, which is the
///   statistic the old test measured. Over 500 pairs: **0** exhausted the budget and the worst side
///   needed 19 of 64 redraws. `P(exhaust) ≈ 0.72^64 ≈ 4e-10` per side, so the search is safe by a
///   factor of about a billion. This asserts that directly, which is the doc's own stated bar —
///   *"bounded rejection sampling terminates comfortably"*.
#[test]
fn mutation_yields_feasible_children_often_enough_for_rejection_sampling() {
    let t = Templates::authored();
    let squad0 = SquadGenome::authored(&t);
    let swarm0 = SwarmGenome::authored(&t);
    let mut rng = seeded(99);

    // Enough draws that the rate is a measurement and not a coin flip.
    const N: u32 = 2000;
    let (mut squad_ok, mut swarm_ok) = (0u32, 0u32);
    for _ in 0..N {
        let squad = mutate_squad(&t, &squad0, &mut rng).expect("mutate");
        let swarm = mutate_swarm(&t, &swarm0, &mut rng).expect("mutate");
        if squad_feasible(&t, &squad).is_ok() {
            squad_ok += 1;
        }
        if swarm_feasible(&t, &swarm).is_ok() {
            swarm_ok += 1;
        }
    }
    let (squad_rate, swarm_rate) = (squad_ok as f32 / N as f32, swarm_ok as f32 / N as f32);
    // Floors at roughly half the measured rate: far enough below to never flap, close enough that a
    // SIGMA large enough to matter trips them.
    assert!(
        squad_rate > 0.45,
        "squad children are feasible only {squad_rate:.3} of the time (measured 0.902) — SIGMA {SIGMA}          may be too large"
    );
    assert!(
        swarm_rate > 0.15,
        "swarm children are feasible only {swarm_rate:.3} of the time (measured 0.281, and this is          the binding side) — SIGMA {SIGMA} may be too large"
    );

    // And the invariant the search actually rests on: bounded rejection sampling terminates, with room
    // to spare. This is what would break if SIGMA drifted, and it is what breaks the search when it
    // does — an exhausted budget is a hard error in `propose_*`, never a silent skip.
    const PAIRS: u32 = 500;
    let (mut exhausted, mut redraws, mut worst) = (0u32, 0u32, 0u32);
    for _ in 0..PAIRS {
        for side in 0..2 {
            let mut rejected = 0u32;
            let got = if side == 0 {
                propose_squad(&t, &squad0, &mut rng, &mut rejected).map(|_| ())
            } else {
                propose_swarm(&t, &swarm0, &mut rng, &mut rejected).map(|_| ())
            };
            if got.is_err() {
                exhausted += 1;
            }
            redraws += rejected;
            worst = worst.max(rejected);
        }
    }
    assert_eq!(
        exhausted, 0,
        "{exhausted} of {} proposals exhausted the {MAX_MUTATION_ATTEMPTS}-draw budget — that is a          hard error in the search, not a slow path",
        PAIRS * 2
    );
    let mean_redraws = redraws as f32 / (PAIRS * 2) as f32;
    assert!(
        mean_redraws < 10.0,
        "a proposal needs {mean_redraws:.1} redraws on average (worst {worst}, budget          {MAX_MUTATION_ATTEMPTS}) — rejection sampling no longer terminates comfortably"
    );
}

#[test]
fn proposal_always_returns_a_feasible_child() {
    // `propose_*` is what makes "a child that reaches evaluation is always loadable" true.
    let t = Templates::authored();
    let squad0 = SquadGenome::authored(&t);
    let swarm0 = SwarmGenome::authored(&t);
    let mut rng = seeded(7);
    let mut rejected = 0;
    for _ in 0..16 {
        let squad = propose_squad(&t, &squad0, &mut rng, &mut rejected).expect("a feasible child");
        let swarm = propose_swarm(&t, &swarm0, &mut rng, &mut rejected).expect("a feasible child");
        assert!(squad_feasible(&t, &squad).is_ok());
        assert!(swarm_feasible(&t, &swarm).is_ok());
    }
    assert!(rejected > 0, "some draws should be rejected — else the guard is not binding");
}
