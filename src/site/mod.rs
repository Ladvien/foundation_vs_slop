//! **Site-67** — the persistent hub the player returns to between expeditions (FVS-G-1 / G-4 / G-5 / D-4).
//!
//! Design: `docs/2026-07-26-site-hub-and-operative-knowledge.md` §2. Read it before adding to this
//! module; several of the shapes here are deliberate reversals of the obvious approach.
//!
//! **Status: kit + layout.** [`pieces`] and [`layout`] are landed; navigation, the door trigger, the
//! aperture shader and the specimen cell bodies are not. What follows is the contract the rest must honour, recorded now so it
//! is not re-derived.
//!
//! ## Why the Site persists for free
//!
//! `session::run_scoped()` is `DespawnOnExit(RunState::Active)`, and its doc already names this module
//! as the exemption. Site entities persist simply by **not carrying it** — there is no exempt-list to
//! maintain and no teardown system to keep in step with every spawner. That is the surviving half of
//! FVS-A-4, and A-5 made it free.
//!
//! ## The Site is entities, NOT a `Dungeon`
//!
//! `Dungeon` is a single resource that A-5 regenerates per run, so a second procedural world would make
//! "which one does this system mean?" ambiguous everywhere. The hub must also be *learnable*, which
//! procedural generation actively fights. So the Site is hand-authored entities with its own small
//! walkable mask, and that mask must have a **different type name** from `Dungeon` so no system can
//! confuse the two.
//!
//! ## The constraint that decides squad presence
//!
//! Squad `Unit`s cannot stand here, and the reason is concrete rather than stylistic: `spawn_unit`
//! carries `run_scoped()`, and both `squad::unit_movement` and `fog::update_los` take `Res<Dungeon>` —
//! which while `Idle` is **absent** (first boot) or **stale** (post-run). Real units at the Site would
//! collide with a ghost dungeon and repaint a ghost fog grid. The Site therefore gets its own
//! `SiteAvatar`, never `squad::Unit`; promoting avatars to real operatives is FVS-G-3's job.
//!
//! ## Determinism
//!
//! The gameplay half (the Site root, the Site↔specimen relationship) is **harness-visible**, because
//! FVS-D-4's acceptance — "specimens accumulate across expeditions" — is otherwise the single most
//! important thing to test and the one thing untestable. That is the same mistake `src/session/`
//! documents about putting the win/lose decision in `AppState`.
//!
//! It cannot move `GOLDEN`, and the reason must survive someone later moving code around: the Site root
//! is **bodiless** (no `Transform`, no `Health`), so it contributes no row to `snapshot_hash` and no
//! actor to `liveness_violations` — exactly `squad::Squad`'s shape and for exactly that reason. The
//! presentation half (geometry, avatars, the aperture material) is windowed-only `SiteVisualsPlugin`,
//! and any body it spawns must carry a `Transform` **without** a `Health` to stay out of the fold.

pub mod aperture;
pub mod layout;
pub mod nav;
pub mod o5;
pub mod pieces;
pub mod visuals;

pub use layout::{AreaId, SiteLayout};
pub use nav::SiteNav;
pub use o5::{Consumable, ExpeditionReport, O5Standing, Rating};
pub use visuals::SiteVisualsPlugin;
pub use pieces::SitePiece;

use bevy::prelude::*;

/// The Site itself — a **bodiless** record, exactly like `squad::Squad`.
///
/// No `Transform` and no `Health`, so it contributes no row to `sim_harness::snapshot_hash` and no actor
/// to `liveness_violations`. That is what lets the gameplay half be harness-visible without touching the
/// pinned core, and it is a property of the *entity shape*, not of where the plugin is registered — so it
/// survives someone later moving this code.
#[derive(Component, Debug)]
pub struct Site;

/// Handle to the one Site entity, so the `Contained` hook can link a specimen without a query.
///
/// A resource rather than a `Query<Entity, With<Site>>` because the reader is a **component hook**
/// running in `DeferredWorld`, where a query would be both awkward and a pick. Push 8 measured resources
/// hash-neutral, so this costs nothing.
#[derive(Resource, Debug, Clone, Copy)]
pub struct SiteRoot(pub Entity);

/// Specimen → Site. The repo's **third** Bevy relationship, after `squad::MemberOf`/`SquadRoster` and
/// `containment::Holding`/`HeldBy`.
#[derive(Component, Debug)]
#[relationship(relationship_target = SiteSpecimens)]
pub struct HeldAt(pub Entity);

/// Site → its specimens.
///
/// **Both gotchas the other two pairs document apply here, and they bite in different places:**
///
/// 1. **Bevy expresses an empty target by REMOVING the component**, so a Site holding nothing matches
///    *nothing* on a bare `Query<&SiteSpecimens>` — which reads as "no Site" rather than "no specimens".
///    Always `Option<&SiteSpecimens>`. This is the first expedition's state, so it is the common case,
///    not an edge case.
/// 2. **Its order is attach order, not a total order.** Anything that picks — assigning specimens to
///    containment cells, choosing one to research — must sort by a stable key first. That key is
///    `Specimen::captured_tick`, which exists for this reason; `(captured_tick, captured)` is total even
///    for a same-tick double capture.
#[derive(Component, Debug)]
#[relationship_target(relationship = HeldAt)]
pub struct SiteSpecimens(Vec<Entity>);

/// The **gameplay** half of Site-67: the persistent root and the specimen relationship. Registered in
/// BOTH `lib::run` and `sim_harness`.
///
/// Harness-visible deliberately. FVS-D-4's acceptance is "specimens accumulate across expeditions", and
/// a windowed-only relationship would make the single most important thing about the roguelite boundary
/// the one thing no test could reach — precisely the mistake `src/session/` records about putting the
/// win/lose decision in `AppState`.
///
/// **`Startup`, not `OnEnter(RunState::Idle)`.** `Idle` is the default state and Bevy runs the initial
/// `StateTransition` *before* `PreStartup` (the same trap `RunState`'s doc records for `Active`), so
/// `OnEnter` would fire before any asset handle existed — and again on every `RETURN TO SITE`,
/// duplicating the Site unless guarded by a bool, which is the two-mechanisms-for-one-invariant shape
/// this codebase rejects. `Startup` runs exactly once per process and needs no guard.
pub struct SitePlugin;

impl Plugin for SitePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_site);
    }
}

/// Create the Site and publish its handle. Note the absence of `session::run_scoped()` — that *is* the
/// persistence mechanism (FVS-A-4's surviving half), not an oversight.
pub(crate) fn spawn_site(mut commands: Commands) {
    let site = commands.spawn(Site).id();
    commands.insert_resource(SiteRoot(site));
}

/// The Site's specimens, **ordered by capture time** — the total order `SiteSpecimens` does not provide.
///
/// Every consumer that assigns cells, picks a research subject, or renders the roster should go through
/// this rather than walking the relationship directly.
pub fn specimens_in_capture_order(
    specimens: &Query<(Entity, &crate::containment::Specimen)>,
    roster: Option<&SiteSpecimens>,
) -> Vec<Entity> {
    let Some(roster) = roster else {
        // Not an error: Bevy removes the target component when the Site holds nothing, which is exactly
        // the state of a first expedition.
        return Vec::new();
    };
    let mut out: Vec<(u64, Entity, Entity)> = roster
        .0
        .iter()
        .filter_map(|&e| specimens.get(e).ok().map(|(e, s)| (s.captured_tick, s.captured, e)))
        .collect();
    // SORT-OK: `(captured_tick, captured)` is total — two specimens banked on the same tick still differ
    // by which anomaly they came from, and one anomaly cannot be captured twice (`Contained` is inserted
    // once and never removed).
    out.sort_unstable_by_key(|(tick, captured, _)| (*tick, *captured));
    out.into_iter().map(|(_, _, e)| e).collect()
}
