//! **The research verb** (FVS-E-5) — the thing that was missing.
//!
//! Push 4 shipped a complete, well-grounded research economy as pure functions and never connected it
//! to a player: `ResearchPosterior::observe` had no gameplay caller, `AuthoredExperiments` was never
//! inserted, and no captured specimen carried a payout. So a posterior never moved during play and
//! `capture → research → unlock` was reachable only from test setup. This module is the missing half.
//!
//! # Where it runs, and why that is safe
//!
//! `Update`, gated on `AppState::Site` — research happens between expeditions, at the bench, never
//! during one. So this adds **no `FixedUpdate` node**, cannot permute the pinned schedule's
//! linearisation, and cannot move the goldens. The harness never enters `AppState::Site`, which is the
//! same boundary `crate::persist` sits behind.
//!
//! # The draw is seeded from the situation, never accumulated
//!
//! An experiment is a *noisy* reading: with effective reliability `r` you observe the truth, otherwise
//! its opposite. That needs a random draw, and FVS-N-8 is this repo's standing lesson about where those
//! must come from — its third cause was a scatter seed held in a `Local<u32>`, so a death's outcome
//! became a function of *how many events the App had ever processed*. One difference anywhere
//! desynchronised everything after it, permanently.
//!
//! So [`draw_seed`] is a pure function of `(specimen identity, parameter, how many times this parameter
//! has been tested)` — all recorded state, no accumulator, no wall clock, no entity id. Re-running the
//! same experiment on the same specimen at the same fatigue level yields the same reading, which is also
//! the right *fiction*: repeating a test you have already run does not resample the universe.
//!
//! # Why a request message rather than mutating from the input handler
//!
//! One writer, the discipline `session::ForceVictory` and `parasite::CureRequest` already use. The key
//! binding lives in the UI layer and only *asks*; [`run_experiments`] is the single place a posterior
//! ever moves in play, so there is one path to audit when a belief does something surprising.

use bevy::prelude::*;

use super::curriculum::Curriculum;
use super::pacing::ExperimentFatigue;
use super::posterior::{HiddenParam, ResearchPosterior, PARAM_COUNT};
use super::unlock::{Researched, TechTree};
use crate::containment::Specimen;
use crate::rng::{seeded, DetRng};

/// How many times each hidden parameter of one specimen has been tested.
///
/// Drives [`ExperimentFatigue`] — the `k`-th test on a parameter runs at `reliability · decay^k`, which
/// is FVS-E-3's authored front-loading. Also the fatigue state a save has to carry, or reloading would
/// hand the player a fresh battery of full-strength tests on a specimen they had already exhausted.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentLog {
    /// Indexed by [`HiddenParam::as_index`].
    pub runs: [u32; PARAM_COUNT],
}

impl ExperimentLog {
    pub fn runs_on(&self, param: HiddenParam) -> u32 {
        self.runs[param.as_index()]
    }
    fn record(&mut self, param: HiddenParam) {
        self.runs[param.as_index()] = self.runs[param.as_index()].saturating_add(1);
    }
}

/// The specimen currently on the slab.
///
/// `None` when nothing is held or nothing is studiable. FVS-L-3's Site screen drives this; until then
/// [`keep_a_study_subject`] keeps it pointed at something sensible so the bench is never inert for a
/// reason the player cannot see.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StudySubject(pub Option<Entity>);

/// "Run the best available test on the studied specimen."
///
/// A request, not a command: the UI asks, [`run_experiments`] decides. If nothing is informative the
/// request is dropped and the readout already says why.
#[derive(Message, Debug, Clone, Copy)]
pub struct RunExperiment;

/// Everything needed to actually perform one test, resolved from the curriculum.
struct Offer {
    param: HiddenParam,
    /// Reliability **after** fatigue — what the Bayesian update is told, so the belief moves by exactly
    /// as much as the weakened test earns.
    reliability: f32,
}

/// The best test still worth running on `posterior`, or `None` if the specimen is exhausted.
///
/// Ranked by expected information gain via [`super::rank_by_information_gain`], which is total (ties
/// break on authored index), then filtered by fatigue: an exhausted parameter is **not offered** rather
/// than offered and inert — `ExperimentFatigue::effective` returns `None` below `USELESS_BELOW`,
/// because a test under 0.5 reliability does not merely fail to inform, it moves the belief the wrong
/// way.
pub fn best_offer(
    experiments: &[super::Experiment],
    posterior: &ResearchPosterior,
    log: &ExperimentLog,
    fatigue: ExperimentFatigue,
) -> Option<HiddenParam> {
    super::rank_by_information_gain(experiments, posterior)
        .into_iter()
        .find_map(|i| {
            let e = &experiments[i];
            if posterior.is_revealed(e.param) {
                return None;
            }
            if e.expected_information_gain(posterior) <= 0.0 {
                return None;
            }
            fatigue.effective(e.reliability, log.runs_on(e.param)).map(|_| e.param)
        })
}

/// Resolve the offer's effective reliability alongside its parameter.
fn resolve_offer(
    experiments: &[super::Experiment],
    posterior: &ResearchPosterior,
    log: &ExperimentLog,
    fatigue: ExperimentFatigue,
) -> Option<Offer> {
    let param = best_offer(experiments, posterior, log, fatigue)?;
    // The authored entry for that parameter — the same one the ranking chose.
    let e = experiments.iter().find(|e| e.param == param)?;
    let reliability = fatigue.effective(e.reliability, log.runs_on(param))?;
    Some(Offer { param, reliability })
}

/// The seed for one experimental reading.
///
/// A pure function of recorded state — `captured_tick` identifies the specimen (it is the same stable
/// key `Specimen` already carries for cell assignment and persistence), the subject and parameter
/// identify the question, and `prior_runs` identifies *which* repeat this is. Deliberately **not** the
/// specimen's `Entity`: that is a process-local arena index, and FVS-N-8 is the standing proof of what
/// happens when an allocated id reaches a seed.
///
/// FNV-1a over the four fields, hand-rolled for the reason `autogib::seed_from_path` gives:
/// `DefaultHasher` is not guaranteed stable across toolchains, so it has no business seeding anything
/// that must reproduce between builds.
pub fn draw_seed(
    captured_tick: u64,
    subject: crate::knowledge::Subject,
    param: HiddenParam,
    prior_runs: u32,
) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut h = OFFSET;
    let mut eat = |v: u64| {
        for b in v.to_le_bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(PRIME);
        }
    };
    eat(captured_tick);
    eat(subject.as_index() as u64);
    eat(param.as_index() as u64);
    eat(u64::from(prior_runs));
    h
}

/// Perform one reading: the truth, flipped with probability `1 - reliability`.
///
/// Split out as a pure function so the noise model is testable without an `App` — and so it is obvious
/// that reliability means exactly one thing here, the same thing `ResearchPosterior::observe` is told.
pub fn read(truth: bool, reliability: f32, seed: u64) -> bool {
    let mut rng = seeded(seed);
    if rng.unit() < f64::from(reliability) {
        truth
    } else {
        !truth
    }
}

/// **The one place a posterior ever moves in play.**
pub fn run_experiments(
    mut requests: MessageReader<RunExperiment>,
    studied: Res<StudySubject>,
    curriculum: Res<Curriculum>,
    tree: Res<TechTree>,
    fatigue: Res<ExperimentFatigue>,
    mut specimens: Query<
        (&Specimen, &mut ResearchPosterior, &mut ExperimentLog),
        Without<Researched>,
    >,
) {
    // Drain regardless: a request that arrives while nothing is studiable is spent, not queued. Holding
    // it would fire later against a different specimen than the player was looking at.
    let asked = requests.read().count();
    if asked == 0 {
        return;
    }
    let Some(entity) = studied.0 else { return };
    let Ok((specimen, mut posterior, mut log)) = specimens.get_mut(entity) else { return };
    if !curriculum.is_available(specimen.subject, &tree) {
        // Gated on prior research. The readout names what it is waiting on; silently doing nothing here
        // is correct because the player has not spent anything.
        return;
    }
    let Some(truth) = curriculum.truth(specimen.subject) else { return };
    let experiments = curriculum.experiments(specimen.subject);
    // One test per request, even if several arrived in a frame: a test is a deliberate act, and
    // collapsing a double key-press into two readings would spend fatigue the player did not intend.
    let Some(offer) = resolve_offer(experiments, &posterior, &log, *fatigue) else { return };
    let seed = draw_seed(specimen.captured_tick, specimen.subject, offer.param, log.runs_on(offer.param));
    let observed = read(truth.get(offer.param), offer.reliability, seed);
    posterior.observe(offer.param, observed, offer.reliability);
    log.record(offer.param);
}

/// Keep [`StudySubject`] pointing at a specimen worth looking at.
///
/// Prefers the most uncertain **available** specimen — the one with work left to do — and falls back to
/// any held specimen so a fully-gated collection still shows the player *something*, with the readout
/// explaining why it cannot be studied. Placeholder for FVS-L-3's selector, and marked as such: the pick
/// is by `(−entropy, captured_tick)`, a total order, so it cannot flicker between frames.
pub fn keep_a_study_subject(
    mut studied: ResMut<StudySubject>,
    curriculum: Res<Curriculum>,
    tree: Res<TechTree>,
    specimens: Query<(Entity, &Specimen, &ResearchPosterior), Without<Researched>>,
) {
    // Still valid? Leave a deliberate selection alone.
    if let Some(e) = studied.0 {
        if specimens.get(e).is_ok() {
            return;
        }
    }
    let mut best: Option<(bool, f32, u64, Entity)> = None;
    for (e, specimen, posterior) in &specimens {
        let available = curriculum.is_available(specimen.subject, &tree);
        let key = (available, posterior.total_entropy(), specimen.captured_tick, e);
        // Total by construction: `captured_tick` breaks the entropy tie and `Entity` breaks a
        // same-tick double capture, which is the same key `Specimen` documents for cell assignment.
        let better = match best {
            None => true,
            Some((ba, be, bt, _)) => {
                (key.0, key.1) > (ba, be) || (key.0 == ba && key.1 == be && key.2 < bt)
            }
        };
        if better {
            best = Some(key);
        }
    }
    let next = best.map(|(_, _, _, e)| e);
    if studied.0 != next {
        studied.0 = next;
    }
}

/// Give every specimen an [`ExperimentLog`] the first frame it is seen.
///
/// A separate pass rather than part of `grant_specimen` deliberately: that hook is on the **pinned**
/// path, and this is bench bookkeeping that only the Site reads. Keeping it here means the fatigue
/// record cannot perturb a capture.
pub fn attach_experiment_logs(
    mut commands: Commands,
    fresh: Query<Entity, (With<Specimen>, Without<ExperimentLog>)>,
) {
    for e in &fresh {
        commands.entity(e).insert(ExperimentLog::default());
    }
}

/// The bench. **Windowed-only**, registered from `lib::run` and never from `sim_harness`.
///
/// Same boundary `crate::persist` sits behind and for the same reason: everything here is gated on
/// `AppState::Site`, which is a UI state the deterministic core deliberately cannot see
/// (`tests/replay.rs::ui_never_leaks_into_deterministic_core` asserts it is absent headless). Research
/// therefore cannot reach `snapshot_hash` by construction rather than by discipline.
pub struct ResearchLabPlugin;

impl Plugin for ResearchLabPlugin {
    fn build(&self, app: &mut App) {
        use crate::ui::state::AppState;
        app.add_message::<RunExperiment>()
            .init_resource::<StudySubject>()
            .init_resource::<ExperimentFatigue>()
            .add_systems(
                Update,
                (
                    // Order matters and is chained rather than left to chance: a specimen banked this
                    // frame gets its log, then a subject is chosen, then the request runs against it.
                    // Unchained, a request could spend itself against last frame's selection.
                    attach_experiment_logs,
                    keep_a_study_subject,
                    run_experiments,
                )
                    .chain()
                    .run_if(in_state(AppState::Site)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::Subject;
    use crate::research::curriculum::{HiddenTruth, ResearchConfig, SubjectResearch};
    use crate::research::unlock::Capability;
    use crate::research::Experiment;

    fn battery() -> Vec<Experiment> {
        HiddenParam::ALL
            .iter()
            .map(|p| Experiment { name: format!("{p:?} ASSAY"), param: *p, reliability: 0.85 })
            .collect()
    }

    fn curriculum() -> Curriculum {
        Curriculum(ResearchConfig {
            subjects: vec![SubjectResearch {
                subject: Subject::ComfortBlob,
                truth: HiddenTruth {
                    lethality: false,
                    contagion: false,
                    capture_basin: true,
                    proliferation: false,
                },
                experiments: battery(),
                unlocks: vec![Capability::MoraleField],
                requires: vec![],
            }],
        })
    }

    #[test]
    fn a_reading_is_the_truth_when_the_test_is_reliable_and_can_be_wrong_when_it_is_not() {
        // The noise model, stated as a property rather than a fixed expectation: a perfect test always
        // reports the truth, and a coin-flip test does not always.
        assert!((0..64).all(|s| read(true, 1.0, s)), "a perfect test must never misreport");
        assert!(
            (0..64).any(|s| !read(true, 0.5, s)),
            "a coin-flip test that never misreports is not modelling noise at all"
        );
    }

    #[test]
    fn the_same_experiment_on_the_same_specimen_reads_the_same_way() {
        // The FVS-N-8 property: the draw is a pure function of recorded state, so it cannot depend on
        // how much has happened in the process. Repeating a test you already ran does not resample the
        // universe — which is the right fiction as well as the safe implementation.
        let a = draw_seed(120, Subject::ComfortBlob, HiddenParam::Lethality, 0);
        let b = draw_seed(120, Subject::ComfortBlob, HiddenParam::Lethality, 0);
        assert_eq!(a, b);
        assert_eq!(read(true, 0.8, a), read(true, 0.8, b));
    }

    #[test]
    fn a_different_repeat_of_the_same_test_is_a_different_reading() {
        // ...but the SECOND run of the same test must be able to disagree with the first, or repeats
        // could never argue with a wrong first impression.
        let first = draw_seed(120, Subject::ComfortBlob, HiddenParam::Lethality, 0);
        let second = draw_seed(120, Subject::ComfortBlob, HiddenParam::Lethality, 1);
        assert_ne!(first, second, "fatigue level must be part of the seed");
    }

    #[test]
    fn distinct_specimens_and_questions_do_not_share_a_reading() {
        let base = draw_seed(120, Subject::ComfortBlob, HiddenParam::Lethality, 0);
        assert_ne!(base, draw_seed(121, Subject::ComfortBlob, HiddenParam::Lethality, 0));
        assert_ne!(base, draw_seed(120, Subject::Parasite, HiddenParam::Lethality, 0));
        assert_ne!(base, draw_seed(120, Subject::ComfortBlob, HiddenParam::Contagion, 0));
    }

    #[test]
    fn the_bench_offers_the_most_informative_test_and_stops_when_exhausted() {
        let exps = battery();
        let p = ResearchPosterior::unknown();
        let log = ExperimentLog::default();
        let fatigue = ExperimentFatigue::default();
        assert!(best_offer(&exps, &p, &log, fatigue).is_some(), "a fresh specimen has work to do");

        // Fatigue every parameter past `USELESS_BELOW`. Nothing may be offered after that — an
        // exhausted test must not be offered-and-inert.
        let spent = ExperimentLog { runs: [9; PARAM_COUNT] };
        assert!(
            best_offer(&exps, &p, &spent, fatigue).is_none(),
            "an exhausted battery must offer nothing rather than a test that would lie"
        );
    }

    #[test]
    fn a_resolved_parameter_is_never_offered_again() {
        let exps = battery();
        let mut p = ResearchPosterior::unknown();
        for q in HiddenParam::ALL {
            p.reveal(q);
        }
        assert!(best_offer(&exps, &p, &ExperimentLog::default(), ExperimentFatigue::default())
            .is_none());
    }

    /// The acceptance FVS-E-5 exists for: running the verb actually moves a belief.
    #[test]
    fn running_the_verb_moves_the_posterior() {
        let mut app = App::new();
        app.add_message::<RunExperiment>()
            .insert_resource(curriculum())
            .init_resource::<TechTree>()
            .init_resource::<ExperimentFatigue>()
            .init_resource::<StudySubject>()
            .add_systems(Update, run_experiments);

        let e = app
            .world_mut()
            .spawn((
                Specimen { captured: Entity::PLACEHOLDER, captured_tick: 120, subject: Subject::ComfortBlob },
                ResearchPosterior::unknown(),
                ExperimentLog::default(),
            ))
            .id();
        app.world_mut().resource_mut::<StudySubject>().0 = Some(e);

        let before = app.world().get::<ResearchPosterior>(e).expect("posterior").belief_entropy();
        app.world_mut().write_message(RunExperiment);
        app.update();
        let after = app.world().get::<ResearchPosterior>(e).expect("posterior").belief_entropy();

        assert!(after < before, "an experiment must reduce uncertainty: {before} -> {after}");
        assert_eq!(
            app.world().get::<ExperimentLog>(e).expect("log").runs.iter().sum::<u32>(),
            1,
            "exactly one test per request"
        );
    }

    #[test]
    fn a_gated_specimen_cannot_be_studied_and_spends_nothing() {
        // The prerequisite gate sits HERE, before the work, rather than on the payout — so a player
        // who studies out of order loses nothing and the readout can tell them what to research first.
        let mut c = curriculum();
        c.0.subjects[0].requires = vec![Capability::FieldCure];

        let mut app = App::new();
        app.add_message::<RunExperiment>()
            .insert_resource(c)
            .init_resource::<TechTree>()
            .init_resource::<ExperimentFatigue>()
            .init_resource::<StudySubject>()
            .add_systems(Update, run_experiments);

        let e = app
            .world_mut()
            .spawn((
                Specimen { captured: Entity::PLACEHOLDER, captured_tick: 7, subject: Subject::ComfortBlob },
                ResearchPosterior::unknown(),
                ExperimentLog::default(),
            ))
            .id();
        app.world_mut().resource_mut::<StudySubject>().0 = Some(e);

        let before = app.world().get::<ResearchPosterior>(e).expect("posterior").belief_entropy();
        app.world_mut().write_message(RunExperiment);
        app.update();

        assert_eq!(
            app.world().get::<ResearchPosterior>(e).expect("posterior").belief_entropy(),
            before,
            "a gated specimen must not be researchable"
        );
        assert_eq!(
            app.world().get::<ExperimentLog>(e).expect("log").runs.iter().sum::<u32>(),
            0,
            "and it must not spend fatigue either"
        );
    }

    /// The whole FVS-E-5 arc, end to end: repeated study resolves a specimen and pays out.
    #[test]
    fn studying_a_specimen_to_completion_grants_its_authored_capability() {
        let mut app = App::new();
        app.add_message::<RunExperiment>()
            .insert_resource(curriculum())
            .init_resource::<TechTree>()
            .init_resource::<ExperimentFatigue>()
            .init_resource::<StudySubject>()
            .add_systems(Update, (run_experiments, super::super::unlock::finish_completed_research));

        let e = app
            .world_mut()
            .spawn((
                Specimen { captured: Entity::PLACEHOLDER, captured_tick: 42, subject: Subject::ComfortBlob },
                ResearchPosterior::unknown(),
                ExperimentLog::default(),
                super::super::unlock::Unlocks(vec![Capability::MoraleField]),
            ))
            .id();
        app.world_mut().resource_mut::<StudySubject>().0 = Some(e);

        // Bounded: if the arc cannot finish within a generous budget that is the bug, not a slow test.
        for _ in 0..200 {
            if app.world().get::<Researched>(e).is_some() {
                break;
            }
            app.world_mut().write_message(RunExperiment);
            app.update();
        }

        assert!(
            app.world().get::<Researched>(e).is_some(),
            "repeated study must eventually resolve a specimen — otherwise the arc has no end"
        );
        assert!(
            app.world().resource::<TechTree>().has(Capability::MoraleField),
            "and completing it must grant the authored capability"
        );
    }
}
