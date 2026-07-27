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

#[test]
fn mutation_yields_feasible_children_often_enough_for_rejection_sampling() {
    // A canary on SIGMA, and a regression test for a bug this test found: with the slope sign left
    // free, `Linear{m}` went negative half the time, `guaranteed_floor` lost both unconditional tails
    // (`wander`, `follow_anchor`, both authored `m = 0.0`), and **0 of 32** children were feasible —
    // the search would have spun forever. `ParamKind::SignLocked` fixed it.
    //
    // The residual rejection rate is the guard working as designed (`wander`'s intercept sits 0.02
    // above MIN_SCORE), and `propose_*` absorbs it by redrawing. The bar here only has to be high
    // enough that bounded rejection sampling terminates comfortably.
    let t = Templates::authored();
    let squad0 = SquadGenome::authored(&t);
    let swarm0 = SwarmGenome::authored(&t);
    let mut rng = seeded(99);
    let mut ok = 0;
    for _ in 0..64 {
        let squad = mutate_squad(&t, &squad0, &mut rng).expect("mutate");
        let swarm = mutate_swarm(&t, &swarm0, &mut rng).expect("mutate");
        if feasible(&t, &squad, &swarm).is_ok() {
            ok += 1;
        }
    }
    assert!(ok >= 16, "only {ok}/64 joint children feasible — SIGMA {SIGMA} is too large");
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
