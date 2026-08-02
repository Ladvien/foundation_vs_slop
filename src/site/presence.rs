//! **Which room is the player standing in** — the hub's missing keystone.
//!
//! # Why this did not exist, and what its absence cost
//!
//! Site-67 shipped twelve areas, and [`AreaId`] was used in exactly two ways: to *look up* a rect for
//! per-wing lighting, and to group staff spawns by post. Nothing ever asked the question the other way
//! round. Grepping every system gated on `AppState::Site` on 2026-08-02 turned up nine player verbs
//! and **not one of them was room-gated** — the archive, the roster, the specimen selector, the shop
//! and the curate button are keyed panels that work identically standing anywhere in the Site,
//! including in a corridor. The single exception was walking into the ASYNC door, which tests an AABB
//! by hand (`visuals::enter_the_door`).
//!
//! So `layout::AreaId`'s own doc comment named the defect — *"the repo's named top process risk is
//! shipping a room with no verb in it"* — and the shape of the fix was upside down. The rooms were
//! not short of verbs so much as the verbs were short of rooms, and nothing in the codebase could
//! tell you where you were.
//!
//! # One resource, one writer, one message
//!
//! [`CurrentArea`] is the answer, [`track_current_area`] is its **only** writer, and [`AreaEntered`]
//! is how everything else finds out. Six panels diffing the resource themselves would be six chances
//! to disagree about when a transition happened; a message is the same single-intent-channel
//! discipline `review::PurchaseRequest` states — *"the click and the key cannot drift apart"*.
//!
//! # Cosmetic by construction
//!
//! Windowed-only, `Update`, and driven off [`PlayerAvatar`] — a body that carries a `Transform` and no
//! `Health`, so it contributes no row to `sim_harness::snapshot_hash`. `AppState` is registered only
//! in the windowed build (`ui::state`), so none of this exists in a headless rollout. It changes what
//! is *offered* to the player, never what the simulation computes.

use bevy::prelude::*;

use super::layout::AreaId;
use super::visuals::{PlayerAvatar, SiteLayoutRes};
use crate::ui::state::AppState;

/// The area the player's avatar is standing in, or `None` for a corridor or the gaps between wings.
///
/// **`None` is a real answer, not a missing one.** The spine is `AreaId::Corridor` and reads as an
/// area; the unfloored gaps between wings are neither, and a hub verb must not fire in one. This is
/// the same distinction `knowledge::Belief` insists on for ignorance — absence is its own state, not
/// a low-confidence version of presence.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CurrentArea(pub Option<AreaId>);

/// The player crossed into `to`, having come from `from`.
///
/// Carries both ends so a reader can close what the old room opened without keeping its own copy of
/// the previous value — which is exactly the duplicated state this message exists to remove.
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AreaEntered {
    pub from: Option<AreaId>,
    pub to: Option<AreaId>,
}

/// Pure half of [`track_current_area`], so the transition logic is testable without an `App`.
///
/// Returns `Some(message)` only on a genuine change. Emitting every frame would make "on entry" mean
/// "continuously", and a panel spawner wired to that would respawn itself sixty times a second.
pub fn transition(was: CurrentArea, now: Option<AreaId>) -> Option<AreaEntered> {
    (was.0 != now).then_some(AreaEntered { from: was.0, to: now })
}

/// Follow the player's avatar and publish the room it is in.
///
/// `With<PlayerAvatar>`, **not** `With<SiteAvatar>` — the distinction `enter_the_door` documents in
/// full and for the same reason. Seven staff walk this hub; if any body could set the current area,
/// the archivist crossing the spine would open the requisition panel.
pub fn track_current_area(
    layout: Option<Res<SiteLayoutRes>>,
    avatars: Query<&Transform, With<PlayerAvatar>>,
    mut current: ResMut<CurrentArea>,
    mut out: MessageWriter<AreaEntered>,
) {
    let Some(layout) = layout else { return };
    // `single()` and not `iter().next()`: exactly one body carries `PlayerAvatar` (`visuals` inserts
    // it on one of the five operatives), and taking "the first" of a query would be a pick with no
    // stable key — the shape `tests/determinism_lint.rs` exists to catch. If that invariant ever
    // breaks, this reads as "no player" rather than as an arbitrary player, which is the failure
    // mode that shows up in a log instead of in a wrong room.
    let Ok(tf) = avatars.single() else {
        return;
    };
    let cell = IVec2::new(
        (tf.translation.x - layout.0.origin.0).floor() as i32,
        (tf.translation.z - layout.0.origin.2).floor() as i32,
    );
    let now = layout.0.area_at(cell);
    if let Some(msg) = transition(*current, now) {
        info!("site: entered {:?} (from {:?})", msg.to, msg.from);
        *current = CurrentArea(now);
        out.write(msg);
    }
}

/// A `run_if` condition: is the player in this room?
///
/// This is what turns a hub screen into a hub *place*. Note it gates **spawning the panel**, never
/// the applier system behind it — the Director's call on 2026-08-02 was that presence *offers* and
/// the key still *acts*, so nothing a player can do today gets harder.
pub fn in_area(area: AreaId) -> impl Fn(Res<CurrentArea>) -> bool + Clone {
    move |current: Res<CurrentArea>| current.0 == Some(area)
}

/// Leaving the Site entirely means the player is nowhere in it — otherwise the last room visited
/// stays "current" across a whole expedition, and the panel it owns reopens on return without the
/// player having walked anywhere.
pub fn clear_current_area(mut current: ResMut<CurrentArea>, mut out: MessageWriter<AreaEntered>) {
    if let Some(msg) = transition(*current, None) {
        *current = CurrentArea(None);
        out.write(msg);
    }
}

pub struct SitePresencePlugin;

impl Plugin for SitePresencePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CurrentArea>()
            .add_message::<AreaEntered>()
            .add_systems(
                Update,
                track_current_area.run_if(in_state(AppState::Site)),
            )
            .add_systems(OnExit(AppState::Site), clear_current_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A transition fires once, on the edge — not every frame the player stands in the room.
    #[test]
    fn entering_a_room_is_an_edge_and_standing_in_it_is_not() {
        let outside = CurrentArea(None);
        assert_eq!(
            transition(outside, Some(AreaId::Records)),
            Some(AreaEntered { from: None, to: Some(AreaId::Records) }),
        );
        // Same room next frame: nothing. A panel spawner wired to a level rather than an edge would
        // despawn and respawn itself sixty times a second, and the button under the cursor with it.
        let inside = CurrentArea(Some(AreaId::Records));
        assert_eq!(transition(inside, Some(AreaId::Records)), None);
        // ...and leaving is an edge too, carrying the room being left so a reader can close it.
        assert_eq!(
            transition(inside, None),
            Some(AreaEntered { from: Some(AreaId::Records), to: None }),
        );
    }

    /// Every room the player can stand in answers `area_at`, and the corridor answers as itself.
    #[test]
    fn the_shipped_layout_names_a_room_for_every_walkable_cell_or_the_spine() {
        let l = crate::site::SiteLayout::load().expect("the shipped layout must load");
        let mut homeless = Vec::new();
        for f in &l.floor {
            for c in f.cells() {
                if l.is_walkable(c) && l.area_at(c).is_none() {
                    homeless.push(c);
                }
            }
        }
        assert!(
            homeless.is_empty(),
            "walkable floor belonging to no area at all — a player standing here is nowhere, and \
             every presence-driven verb goes quiet: {homeless:?}",
        );
    }

    /// The gate is exact. A verb offered in Records must not also be offered in the room next door.
    ///
    /// Driven through a real schedule rather than by calling the closure, because what is being
    /// asserted is that it works as a **run condition** — the thing every panel will hang off.
    #[derive(Resource, Default)]
    struct Ran(u32);

    #[test]
    fn the_area_gate_admits_one_room_and_no_other() {
        let mut app = App::new();
        app.init_resource::<Ran>()
            .insert_resource(CurrentArea(None))
            .add_systems(
                Update,
                (|mut r: ResMut<Ran>| r.0 += 1).run_if(in_area(AreaId::Records)),
            );

        app.update();
        assert_eq!(app.world().resource::<Ran>().0, 0, "nowhere is not Records");

        app.insert_resource(CurrentArea(Some(AreaId::Research)));
        app.update();
        assert_eq!(
            app.world().resource::<Ran>().0,
            0,
            "the room next door must not open Records' panel"
        );

        app.insert_resource(CurrentArea(Some(AreaId::Records)));
        app.update();
        assert_eq!(app.world().resource::<Ran>().0, 1, "standing in Records opens it");
    }
}
