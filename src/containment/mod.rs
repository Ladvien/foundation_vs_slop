//! **Containment — the capture verb.**
//!
//! The pivot this whole backlog is organised around: an anomaly is *contained*, not killed. Capture
//! means driving its local stigmergy fields into a **containable basin** and holding them there while a
//! timer completes — never HP depletion. Killing stays possible and yields nothing, which Push 2's
//! outcome hooks (FVS-B-4) will enforce with a component hook rather than a branch.
//!
//! Shipped so far:
//! * [`rule`] — the data model (FVS-B-1). Pure data + a pure predicate, no ECS, unit-testable without
//!   an `App`.
//! * [`state`] — the phase machine, the `FixedUpdate` tick that runs a rule against the live fields,
//!   and the `on_add` hook that grants a specimen (FVS-B-2 / B-3 / B-4).
//! * [`device`] — archetype 1, the thrown single-target capture device, plus the device↔anomaly
//!   relationship (FVS-B-5 / D-3).
//! * [`area`] — archetype 2 (area-denial quarantine, B-6) and archetype 3 (source elimination /
//!   structure capping, B-7 — which yields a secured flag and deliberately **no** specimen).
//!
//! Still to come in this push: the containment HUD (L-1).

use bevy::prelude::*;

pub mod area;
/// Windowed-only feedback for the quarantine verb. Not registered by [`ContainmentPlugin`] — it is
/// presentation, and the harness must never see a `Gizmos` system.
pub mod cordon;
pub mod device;
pub mod extraction;
pub mod rule;
pub mod state;
pub mod verbs;

pub use area::{Capped, Quarantinable, Quarantine, SiteSecured};
pub use extraction::ExtractionZone;
pub use verbs::{ArmedTool, DeviceSupply, QuarantineSupply, TargetId, TargetSeq};

/// The `containment:` config slice — one authored [`ContainmentRule`] per capturable anomaly.
///
/// **A top-level slice, deliberately outside [`crate::config::WorldConfig`]** — i.e. the offline search
/// does not evolve it, for the same reason `session::SessionConfig` is excluded: a containment rule
/// defines what capturing that anomaly *means*, so a search free to retune it would be moving the
/// objective rather than solving it.
///
/// There is a second, purely mechanical reason worth stating so nobody "fixes" it by accident:
/// `WorldConfig` is `Copy`, and a rule owns a `Vec<FieldCondition>`. Wiring rule *thresholds* into QD as
/// a difficulty axis is a defensible future move — but it needs that `Copy` constraint addressed first,
/// not a silent `Clone` bolted on.
#[derive(Debug, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContainmentConfig {
    /// SCP-999's befriend basin (FVS-C-2).
    pub scp999: ContainmentRule,
    /// SCP-1048's out-watch basin (FVS-C-3).
    pub scp1048: ContainmentRule,
    /// The watch feed (FVS-C-7) — the `AtMost` inverse of `scp1048`'s rule above. Contained by
    /// depriving it of an audience, which is the same channel with the sign flipped.
    pub broadcast: ContainmentRule,
    /// SCP-150's rule (FVS-C-4).
    ///
    /// Carried for the *specimen*, not for a basin: the parasite is extracted by curing its host
    /// (`parasite::cure_infested_hosts`), not by driving a field. It still needs a rule so the research
    /// layer has one to read, and so `unmet()` has something to render if the HUD ever shows it.
    pub scp150: ContainmentRule,
    /// SCP-610's area-denial rule — the first user of the [`area::Quarantine`] archetype.
    ///
    /// Unlike the others this is not a basin the player drives a field into: `tick_quarantine` opens
    /// and closes the attempt on *geometry* (is the bloom inside a quarantine), and this rule is what
    /// `tick_containment` then holds them to while it is open.
    pub scp610: ContainmentRule,
    /// **Does an operative have to KNOW a procedure before the HUD will show it?** (FVS-O-2's benefit
    /// half.)
    ///
    /// Ships `false`. The design doc's claim is that knowledge "is the only thing that makes
    /// containment legible", and this is the switch that makes that literal — but turning it on makes
    /// the *first* encounter with every anomaly a blind one, which is a real difficulty and pacing
    /// decision rather than a wiring detail. So it is wired, inert, and one edit away.
    ///
    /// It belongs in this slice rather than `sim:` because it changes what containment *means* to the
    /// player, not how hard it is — the same line `ContainmentConfig` already draws.
    #[serde(default)]
    pub require_knowledge_for_rules: bool,
}

impl ContainmentConfig {
    /// Reject malformed authored rules at load — one path, no fallback.
    pub fn validate(&self) -> Result<(), String> {
        self.scp999.validate().map_err(|e| format!("containment.scp999: {e}"))?;
        self.scp1048.validate().map_err(|e| format!("containment.scp1048: {e}"))?;
        self.scp150.validate().map_err(|e| format!("containment.scp150: {e}"))?;
        // ⚠️ `scp610` was MISSING here from the day it was added (found 2026-07-30, FVS-K-1). Every
        // other rule was validated at load and 610's was not, so a malformed 610 rule — an unknown
        // channel, a non-finite threshold, a zero hold — would have loaded silently and failed later
        // as "containment never completes", which is the hardest possible symptom to trace back to a
        // typo in a config file. One path, no fallback: it fails at the loader like its siblings.
        self.scp610.validate().map_err(|e| format!("containment.scp610: {e}"))
    }
}
pub use device::{ContainmentDevice, HeldBy, Holding};
pub use rule::{ContainmentRule, FieldCondition, OnBreak, Sign};
pub use state::{Containment, Contained, Phase, Specimen};

/// Registered in **both** `lib::run` and `sim_harness` — containment is pinned gameplay, so the
/// exact-hash gate must cover it.
#[derive(bevy::prelude::Resource, Debug, Clone)]
pub struct ContainmentRules(pub ContainmentConfig);

pub struct ContainmentPlugin;

impl Plugin for ContainmentPlugin {
    fn build(&self, app: &mut App) {
        // Ordered after the AI phase so a tick's containment reads the field this tick's deposits and
        // evaporation already settled — the same "read settled state" edge `squad::unit_movement` uses
        // against `AiSet::Think`. Without it the rule would evaluate against a half-updated grid whose
        // contents depend on schedule accident.
        // Already validated by `config::load_game_config` — the single validation seam. This used to
        // validate-and-panic here instead, which was a second path: a malformed rule would be rejected
        // in the windowed build's plugin build but sail past any consumer that loaded the config
        // without adding this plugin. Validation belongs at the door, once.
        let rules = app.world().resource::<crate::config::GameConfig>().containment.clone();
        app.insert_resource(ContainmentRules(rules));
        app.add_systems(
            FixedUpdate,
            (
                // Open attempts first, so a device thrown or a region entered this tick begins its
                // capture before the tick that evaluates it — neither should cost a tick of progress.
                (device::deploy_devices, area::tick_quarantine),
                state::tick_containment.after(crate::ai::AiSet::FieldUpdate),
                // Release last, so a capture that completes this tick drops its device on the same tick.
                device::release_finished_devices,
            )
                .chain().distributive_run_if(in_state(crate::session::RunState::Active)),
            )
        .init_resource::<area::SiteSecured>()
        .add_systems(FixedUpdate, area::track_secured_sites.distributive_run_if(in_state(crate::session::RunState::Active)))
        // The extraction point, placed on the insertion cell. `RunBuild::Populate` because it reads
        // `Dungeon`, which only exists after `RunBuild::World`. Not a `FixedUpdate` node, so it cannot
        // permute the pinned schedule's linearisation.
        .add_systems(
            OnEnter(crate::session::RunState::Active),
            extraction::spawn_extraction_zone.in_set(crate::session::RunBuild::Populate),
        )
        // The player's verbs. Data and supplies live here (harness-visible, because `deploy_devices`
        // and `tick_quarantine` are pinned and the harness must be able to drive them); the mouse
        // handling lives in `crate::selection`, which is windowed-only by construction.
        .init_resource::<verbs::ArmedTool>()
        .init_resource::<verbs::DeviceSupply>()
        .init_resource::<verbs::QuarantineSupply>()
        .init_resource::<verbs::TargetSeq>()
        // Before the world is built, like `session::reset_run` — a fresh expedition starts with a full
        // pouch, nothing armed, and weapons free.
        .add_systems(
            OnEnter(crate::session::RunState::Active),
            verbs::reset_verbs.before(crate::session::RunBuild::World),
        );
    }
}
