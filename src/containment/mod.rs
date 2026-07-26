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
pub mod device;
pub mod rule;
pub mod state;

pub use area::{Capped, Quarantinable, Quarantine, SiteSecured};

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
}

impl ContainmentConfig {
    /// Reject malformed authored rules at load — one path, no fallback.
    pub fn validate(&self) -> Result<(), String> {
        self.scp999.validate().map_err(|e| format!("containment.scp999: {e}"))
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
        let rules = app.world().resource::<crate::config::GameConfig>().containment.clone();
        rules.validate().unwrap_or_else(|e| panic!("containment config: {e}"));
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
                .chain(),
        )
        .init_resource::<area::SiteSecured>()
        .add_systems(FixedUpdate, area::track_secured_sites);
    }
}
