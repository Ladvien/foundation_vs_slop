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
pub mod perception;
pub mod persona;
pub mod policy;
pub mod qd;
pub mod rl;
pub mod role;

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
                ),
            )
            // Dialogue generation is cosmetic — it turns emitted observations into lines on `Update`
            // (never pinned; a line's text must not enter `snapshot_hash`).
            .add_systems(Update, dialogue::generate_dialogue);
    }
}

/// Insert the role-brain registry, overlaying any `assets/config/roles.ron` overrides.
fn init_role_brains(mut commands: Commands) {
    let mut brains = RoleBrains::defaults();
    match std::fs::read_to_string("assets/config/roles.ron") {
        Ok(src) => match role::parse_roles_ron(&src) {
            Ok(defs) => brains.overlay(defs),
            // Malformed override is a loud error, not a silent fallback — the author asked for a change
            // and must see it failed. The defaults still load so the game runs.
            Err(e) => error!("assets/config/roles.ron is malformed, using role defaults: {e}"),
        },
        // No override file → defaults only (the expected common case; not logged as an error).
        Err(_) => {}
    }
    commands.insert_resource(brains);
}
