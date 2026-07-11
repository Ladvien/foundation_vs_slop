//! Squad AI — SCP Mobile-Task-Force **roles** on top of the shared dual-utility engine (`crate::ai`).
//!
//! The engine (`ai::utility` decide + `ai::brain` glue) already drives crabs and the boss; this plugin
//! gives the player squad the same treatment, specialised into stereotyped-but-customisable roles, plus
//! the squad-only concerns the creatures don't need:
//!   - **[`role`]** — the five role repertoires (data literals + `roles.ron` overrides).
//!   - **[`persona`]** — speaker identity for dialogue.
//!   - **[`policy`]** — the `(Observation, Action)` seam so a learned controller can replace the
//!     hand-authored brain (RL-readiness; Bergdahl et al. 2021, Wu et al. 2019).
//!
//! Design lineage is the same layered-hybrid as `ai::mod` (Dill utility + Colledanchise & Ögren
//! modularity + Reynolds/Game-AI-Pro group navigation for cohesion). Perception, decision, cohesion,
//! actions, dialogue, and the RL/QD harness land as the plugin grows; this module currently wires the
//! role/persona data model and the policy seam.

use bevy::prelude::*;

pub mod actions;
pub mod cohesion;
pub mod dialogue;
/// Rollout evaluation and the offline co-evolutionary search need the headless harness, which is
/// `test-harness`-only. Nothing here ships in the game binary.
#[cfg(feature = "test-harness")]
pub mod coevolve;
#[cfg(feature = "test-harness")]
pub mod evaluate;
/// The standalone level-generation MAP-Elites search + readable `elites_levels.ron` handoff. Gated
/// because it reuses `coevolve::Population`; the genome/quality/eval it drives are all ungated.
#[cfg(feature = "test-harness")]
pub mod level_search;
/// Multi-process fan-out for the offline search: a pool of `train worker` subprocesses evaluate rollouts
/// in parallel (the only determinism-safe axis, since the harness pins each process to one thread).
#[cfg(feature = "test-harness")]
pub mod parallel;
pub mod genome;
/// The level-config genome (dungeon architecture + furniture amount + mushroom amount) the offline
/// search evolves as a fourth, standalone population under a static level-quality objective. Pure logic
/// like `genome`/`world_genome`; GPU-free and unit-testable without the harness.
pub mod level_genome;
/// Static, GPU-free structural metrics scoring a generated level (connectivity, coverage, furniture
/// occupancy, mushroom distribution) — the level search's fitness. Pure logic.
pub mod level_quality;
/// Generate-and-measure evaluator: decode a level genome, run the pure `Dungeon::generate` / `furnish_all`
/// / `habitat::build` pipeline, and score it with `level_quality`. GPU-free, deterministic.
pub mod level_eval;
pub mod perception;
pub mod persona;
pub mod policy;
pub mod qd;
pub mod rl;
pub mod role;
pub mod surprise;
pub mod trace;
/// The world-config genome (field-propagation + sim-dynamics tuning as a flat vector) the offline search
/// evolves as a third population. Pure logic like `genome`; the harness installs a decoded `WorldConfig`.
pub mod world_genome;

use cohesion::{SquadAnchor, SquadControlMode};
use dialogue::{ActiveDialogueProvider, SquadLine, SquadUtterance};
use policy::ActivePolicy;
use role::RoleBrains;

pub struct SquadAiPlugin;

impl Plugin for SquadAiPlugin {
    fn build(&self, app: &mut App) {
        app
            // Selected decision policy (default = the hand-authored dual-utility role brain) and the
            // control mode (default = fully autonomous; a one-line change flips to between-orders).
            .init_resource::<ActivePolicy>()
            .init_resource::<SquadControlMode>()
            .init_resource::<SquadAnchor>()
            // Dialogue provider (default = deterministic template; LLM is opt-in) + RL/QD data.
            .init_resource::<ActiveDialogueProvider>()
            .init_resource::<rl::TrajectoryLog>()
            .init_resource::<rl::Visitation>()
            .init_resource::<trace::Recording>()
            .add_message::<SquadUtterance>()
            .add_message::<SquadLine>()
            // The role repertoires are built once at startup (mirrors `ai::brain::init_brains`). RON
            // overrides from `assets/config/roles.ron` are overlaid here when present — a missing file
            // is not an error (the code-literal defaults are a complete, playable set), but a malformed
            // file fails loudly rather than silently shipping bad brains (no fallback path).
            .add_systems(Startup, init_role_brains)
            // Pinned squad AI on `FixedUpdate`: recompute the group anchor before the squad decides,
            // decide + resolve movement goals in `AiSet::Think` (after the fog LOS this tick, like the
            // creature `think`), then execute the chosen action + medic healing (which read the cached
            // decision, so they run after `Think`). `squad::unit_movement` consumes the `DesiredMove`.
            .add_systems(
                FixedUpdate,
                (
                    cohesion::update_anchor.before(crate::ai::AiSet::Think),
                    perception::squad_think
                        .in_set(crate::ai::AiSet::Think)
                        .after(crate::fog::LosWritten),
                    actions::unit_actions.after(crate::ai::AiSet::Think),
                    actions::medic_heal.after(crate::ai::AiSet::Think),
                    // Episode recording for the offline behaviour search. Disabled by default (one
                    // early return per tick); `squad_ai::evaluate` enables it headlessly. After
                    // `AiSet::Think` so both the creature and squad `think` systems have written
                    // `ActiveBehavior` this tick.
                    trace::record_decisions.after(crate::ai::AiSet::Think),
                    trace::record_outcome.after(crate::ai::AiSet::Think),
                ),
            )
            // Dialogue generation is cosmetic — it turns emitted observations into lines on `Update`
            // (never pinned; a line's text must not enter `snapshot_hash`).
            .add_systems(Update, dialogue::generate_dialogue);
    }
}

/// Insert the role-brain registry, overlaying any `assets/config/roles.ron` overrides. A missing file
/// is the normal case (the code-literal defaults are a complete, playable set); a *present but
/// malformed or invalid* file is a loud startup panic, never a silent fallback to defaults — the
/// author asked for a change and running the game with their override quietly discarded is exactly the
/// "magic results that are hard to debug" the one-path rule forbids (mirrors `config::ConfigPlugin`).
fn init_role_brains(mut commands: Commands, source: Res<crate::ai::brain::BrainSource>) {
    let brains = load_role_brains(&source).unwrap_or_else(|e| panic!("roles.ron: {e}"));
    commands.insert_resource(brains);
}

/// Build the role-brain registry: defaults, overlaid by a validated `assets/config/roles.ron` when
/// present. Returns an error (never a silent default) if the file exists but is malformed or authors an
/// unsafe brain (empty behaviours / out-of-range drive index — see [`role::validate_role_defs`]).
fn load_role_brains(source: &crate::ai::brain::BrainSource) -> Result<RoleBrains, String> {
    // A candidate from the offline search replaces the repertoires wholesale; it never overlays the file,
    // because an evaluation must run exactly the brain the search proposed. It still passes through the
    // identical validation loop below — one gate, one path.
    if let crate::ai::brain::BrainSource::Candidate(candidate) = source {
        // Every role must be supplied. `RoleBrains::overlay` only *inserts* the roles it is given, so a
        // candidate missing one would silently run the authored default for it and the evaluation would be
        // part-authored without saying so — exactly the silent fallback the one-path rule forbids.
        for role in role::RoleId::ALL {
            if !candidate.roles.contains_key(&role) {
                return Err(format!(
                    "candidate brain omits role {role:?}; a candidate must supply all {} roles, or the \
                     evaluation would silently run the authored default for the missing one",
                    role::RoleId::ALL.len()
                ));
            }
        }
        let mut brains = RoleBrains::defaults();
        brains.overlay(
            candidate
                .roles
                .iter()
                .map(|(role, behaviors)| (*role, role::RoleDef { behaviors: behaviors.clone() }))
                .collect(),
        );
        return validated(brains);
    }

    let mut brains = RoleBrains::defaults();
    // A present file is parsed, validated, and overlaid; an *absent* file leaves the defaults untouched
    // (the expected common case — not an error). Any other io error (permission denied,
    // path-is-a-directory) means an override exists and we could not read it — that must fail loudly,
    // exactly as this function's contract promises. `if let Ok(..)` would have swallowed it.
    match std::fs::read_to_string("assets/config/roles.ron") {
        Ok(src) => {
            let defs = role::parse_roles_ron(&src).map_err(|e| format!("malformed: {e}"))?;
            role::validate_role_defs(&defs)?;
            brains.overlay(defs);
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("unreadable: {e}")),
    }
    validated(brains)
}

/// Validate the FINAL repertoire of every role — whether it kept its code-literal default, was replaced
/// from RON, or came from the offline search. `validate_role_defs` only sees RON overrides, so the
/// defaults (and every candidate) would otherwise go unchecked. **This is the one gate every brain
/// passes through**, and it doubles as the genome-level minimal criterion of `squad_ai::genome`.
///
/// 1. An unconditional behaviour (the `follow_anchor` tail) must clear MIN_SCORE, or `decide` would find
///    no eligible bucket and fall through to behaviour 0 — the rank-4 DUTY for a role brain, silently
///    making the unit examine/heal/breach instead of standing down.
/// 2. Ranks must be unique, or `decide`'s weighted-random re-roll makes the unit thrash between modes.
fn validated(brains: RoleBrains) -> Result<RoleBrains, String> {
    for role in role::RoleId::ALL {
        let behaviors = &brains.get(role).behaviors;
        let who = format!("role {role:?}");
        crate::ai::utility::validate_unconditional_default(behaviors, &who)?;
        role::validate_rank_ladder(role, behaviors)?;
    }
    Ok(brains)
}
