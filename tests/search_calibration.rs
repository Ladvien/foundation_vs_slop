//! Calibration gate for the offline behaviour search (feature `test-harness`, GPU-free).
//!
//! The search's thresholds are only meaningful relative to the game as shipped. Two invariants keep them
//! honest, and both are easy to break by editing gameplay rather than the search:
//!
//! 1. **The behavioural minimal criterion must admit the authored brains.** If the shipped squad no longer
//!    produces an encounter in an evaluation episode — nobody is hurt, no crab dies, the map is barely
//!    walked — then `minimal_criterion` rejects every candidate, the archive stays empty, and the search
//!    silently does nothing. That failure is invisible without this test: `train evolve` reports "0 elites"
//!    and exits successfully.
//!
//! 2. **The authored brains must be self-consistent.** A candidate is scored by KL divergence from the
//!    baseline prior; if the reference brain cannot even be encoded and decoded back to itself, every
//!    score is measured against the wrong thing.
//!
//! These are slow (each rollout boots the real game and steps 60 s of simulated time), so they live here
//! rather than in the fast deterministic-core lane.
#![cfg(feature = "test-harness")]

use foundation_vs_slop::sim_harness::serial_guard;
use foundation_vs_slop::squad_ai::coevolve::{
    brains_of, feasible, squad_descriptor, swarm_descriptor, SquadGenome, SwarmGenome, Templates,
    HELD_IN_SEEDS,
};
use foundation_vs_slop::squad_ai::evaluate::rollout;
use foundation_vs_slop::squad_ai::surprise::{minimal_criterion, witnessed_fraction};

/// The calibrated episode length. Below this the authored squad takes no damage on some worlds (measured),
/// so the criterion's "nothing was at stake" clause rejects the shipped game itself.
const EPISODE_TICKS: u32 = 7200;

/// The search's held-in worlds — sourced from `coevolve::HELD_IN_SEEDS`, never re-spelled here: a stale copy
/// would validate the shipped brains on worlds the search no longer runs (exactly the 0xA11CE/0xBEEF trap the
/// single-source constant exists to kill). The gate checks all of them: resting the whole calibration on one
/// seed means an unrelated dungeon-generation tweak can red the build with no other signal.
const WORLDS: [u64; 3] = HELD_IN_SEEDS;

#[test]
fn the_authored_brains_produce_a_real_encounter_on_every_world() {
    // No `serial_guard()` here: `rollout` acquires it for each episode's App lifetime (see
    // `evaluate::rollout`). Taking it here too would lock the non-reentrant `HARNESS_LOCK` twice on this
    // thread and deadlock — `--test-threads=1` already serializes tests within the process.
    let t = Templates::authored();
    let squad = SquadGenome::authored(&t);
    let swarm = SwarmGenome::authored(&t);
    assert!(feasible(&t, &squad, &swarm).is_ok(), "the shipped brains must be loadable");

    for world in WORLDS {
        let brains = brains_of(&t, &squad, &swarm).expect("decode the authored pairing");
        let r = rollout(brains, None, None, None, world, EPISODE_TICKS);

        // The synthetic player must actually drive the squad. Without it the squad idles at spawn.
        assert!(
            r.outcome.cells_covered > 50,
            "world 0x{world:X}: the synthetic player barely moved the squad ({} cells) — is \
             `evaluate::tour_goals` broken?",
            r.outcome.cells_covered
        );
        assert!(!r.trace.is_empty(), "no decisions recorded — is `trace::record_decisions` registered?");

        // And it must RELEASE it: a standing `MoveOrder` overrides locomotion and excludes the unit from
        // `unit_actions` and `medic_heal`, so a permanently-ordered squad evaluates nothing.
        assert!(
            r.outcome.ordered_ticks < EPISODE_TICKS,
            "world 0x{world:X}: the squad was under player order for the whole episode — the squad AI \
             never ran, so the search would be optimising a brain it never exercised"
        );

        minimal_criterion(&r.outcome).unwrap_or_else(|why| {
            panic!(
                "world 0x{world:X}: the SHIPPED brains fail the behavioural minimal criterion ({why}).\n\
                 Every candidate will be rejected and `train evolve` will silently produce an empty \
                 archive and exit 0.\n\
                 Either gameplay changed, or a threshold in `surprise::minimal_criterion` needs \
                 recalibrating against `train probe`.\n\
                 outcome: {:?}",
                r.outcome
            )
        });
    }
}

#[test]
fn the_recorder_sees_both_sides_and_the_witness_filter_bites() {
    // No `serial_guard()` here — `rollout` takes it per episode; double-locking the non-reentrant
    // `HARNESS_LOCK` on one thread would deadlock (see the note above).
    let t = Templates::authored();
    let squad = SquadGenome::authored(&t);
    let swarm = SwarmGenome::authored(&t);
    let brains = brains_of(&t, &squad, &swarm).expect("decode");
    let r = rollout(brains, None, None, None, WORLDS[0], EPISODE_TICKS);

    // Both populations must appear, or a co-evolutionary descriptor is silently zero.
    let squad_d = squad_descriptor(&r.trace, &r.outcome);
    let swarm_d = swarm_descriptor(&r.trace);
    assert!(squad_d.exploration > 0.0, "no squad exploration recorded");
    assert!(
        r.trace.decisions.iter().any(|d| matches!(
            d.context.actor,
            foundation_vs_slop::squad_ai::surprise::ActorKind::Crab
                | foundation_vs_slop::squad_ai::surprise::ActorKind::Scout
                | foundation_vs_slop::squad_ai::surprise::ActorKind::Smiley
        )),
        "no creature decisions recorded — the swarm archive would never fill"
    );
    // `swarm_descriptor` reuses the second axis for persistence; both axes must be finite and in range.
    assert!((0.0..=1.0).contains(&swarm_d.aggression));
    assert!((0.0..=1.0).contains(&swarm_d.exploration));

    // The witness filter must be *doing something*: fog hides part of the swarm, so a real episode is
    // neither fully witnessed nor fully unwitnessed. A W of exactly 1.0 would mean `fog::visible_at` is
    // always true and the inverted-Hunicke constraint is vacuous.
    let w = witnessed_fraction(&r.trace);
    assert!(w > 0.0, "nothing was witnessed — fitness would be identically zero");
    assert!(w < 1.0, "everything was witnessed — the witness filter is not binding");
}

#[test]
fn the_authored_pairing_round_trips_through_the_genome() {
    // The baseline prior is swept from the authored brains; a candidate is scored against it. If encoding
    // and decoding the reference is lossy, every surprise score is measured from the wrong origin.
    use foundation_vs_slop::ai::brain::BrainSource;
    let t = Templates::authored();
    let squad = SquadGenome::authored(&t);
    let swarm = SwarmGenome::authored(&t);
    let BrainSource::Candidate(decoded) = brains_of(&t, &squad, &swarm).expect("decode") else {
        panic!("brains_of must yield a Candidate");
    };
    for (i, role) in foundation_vs_slop::squad_ai::role::RoleId::ALL.iter().enumerate() {
        let rebuilt = decoded.roles.get(role).expect("every role present");
        let authored = &t.roles[i];
        assert_eq!(rebuilt.len(), authored.len(), "{role:?} behaviour count");
        for (a, b) in authored.iter().zip(rebuilt) {
            assert_eq!(a.rank, b.rank, "{role:?} rank");
            assert_eq!(a.mode, b.mode, "{role:?} mode");
            let ac: Vec<_> = a.considerations.iter().map(|c| c.curve).collect();
            let bc: Vec<_> = b.considerations.iter().map(|c| c.curve).collect();
            assert_eq!(ac, bc, "{role:?} curves must survive the genome round trip");
        }
    }
    assert_eq!(decoded.crab.len(), t.crab.len());
    assert_eq!(decoded.scout.len(), t.scout.len());
    assert_eq!(decoded.smiley.len(), t.smiley.len());
}

#[test]
fn recording_does_not_perturb_the_deterministic_core() {
    // `record_outcome` mutates two resources when enabled (`Recording`, `Visitation`). No pinned system
    // reads either *today*, so the recorder is snapshot-neutral — but nothing pinned that, and
    // `deterministic_core_is_bit_identical` cannot: it runs recording OFF in both arms, so the disabled
    // early-return masks the entire enabled path.
    //
    // The day someone wires `Visitation`'s novelty reward into a drive — precisely what `squad_ai::rl`
    // exists for — the offline search would silently evaluate a different game than the one that ships.
    use foundation_vs_slop::sim_harness::{build_headless_app, snapshot_hash, step, SimConfig};
    use foundation_vs_slop::squad_ai::trace::Recording;

    const TICKS: u32 = 600;
    let _serial = serial_guard();
    let cfg = SimConfig::deterministic_core_seeded(WORLDS[0]);

    let quiet = {
        let mut app = build_headless_app(&cfg);
        step(&mut app, &cfg, TICKS);
        snapshot_hash(&mut app)
    };
    let recording = {
        let mut app = build_headless_app(&cfg);
        app.world_mut().resource_mut::<Recording>().start();
        step(&mut app, &cfg, TICKS);
        let hash = snapshot_hash(&mut app);
        assert!(
            !app.world().resource::<Recording>().trace.is_empty(),
            "the recorder was enabled but captured nothing — this test would pass vacuously"
        );
        hash
    };
    assert_eq!(quiet, recording, "enabling the episode recorder changed the pinned simulation state");
}

#[test]
fn a_candidate_genome_actually_changes_the_simulation() {
    // Every other test runs the *authored* brains, decoded into a `Candidate`. If `BrainSource::Candidate`
    // were silently dropped on the floor and the defaults used instead, they would all still pass — and the
    // entire search would be optimising a brain that never reaches `decide()`.
    use foundation_vs_slop::rng::seeded;
    use foundation_vs_slop::sim_harness::{build_headless_app, snapshot_hash, step, SimConfig};
    use foundation_vs_slop::squad_ai::coevolve::mutate_squad_feasible;

    const TICKS: u32 = 600;
    let _serial = serial_guard();
    let t = Templates::authored();
    let swarm = SwarmGenome::authored(&t);

    let hash_of = |squad: &SquadGenome| {
        let cfg = SimConfig::deterministic_core_seeded(WORLDS[0])
            .with_brains(brains_of(&t, squad, &swarm).expect("decode"));
        let mut app = build_headless_app(&cfg);
        step(&mut app, &cfg, TICKS);
        snapshot_hash(&mut app)
    };

    let authored = SquadGenome::authored(&t);
    // Draw the child the SEARCH would propose — `mutate_squad_feasible` IS the co-evolution's
    // `propose_squad`: mutate at `SIGMA`, redraw until `squad_feasible` passes.
    //
    // This used to call a `mutate_squad_for_test` helper at `sigma = 1.0` with **no feasibility check**,
    // which is a second squad-mutation path the search does not have. It could hand `decide()` a brain the
    // search would never propose, and eventually did: the draw produced an Engineer whose every behaviour is
    // gated off, `validate_unconditional_default` correctly refused it, and this test panicked inside
    // `init_role_brains` rather than testing anything. The helper is gone; `squad_feasible` → `is_feasible`
    // → `validate_unconditional_default` is the very invariant that was firing, so the search's own path
    // cannot reproduce it. Testing what the search evaluates is also the point (cf. `mutate_swarm_feasible`).
    let mut rng = seeded(0xD1FF);
    let mutant = mutate_squad_feasible(&t, &authored, &mut rng).expect("a feasible squad child");
    assert_ne!(authored, mutant, "the mutation did nothing");

    assert_ne!(
        hash_of(&authored),
        hash_of(&mutant),
        "a mutated candidate produced an identical world — `BrainSource::Candidate` is not reaching \
         `utility::decide`, so the search is optimising a brain the simulation never runs"
    );
}
