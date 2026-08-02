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

/// **Claim the resources a room-gated panel reads, so its run condition cannot panic.**
///
/// A missing `Res<T>` in Bevy 0.19 does not skip a system — it **panics on parameter validation**,
/// and a run condition is validated like any other system. So the moment [`panel_wanted`] went onto
/// `RecordsPlugin`, `O5Plugin` and friends, every one of them acquired a hard dependency on a
/// resource that only `SitePresencePlugin` inserts.
///
/// That is invisible in the shipped game, because `lib::run` registers all of them together. It is
/// **not** invisible to a test that builds a subset — `tests/replay.rs`'s
/// `returning_to_the_site_after_a_run_does_not_panic` mirrors the windowed plugin list *without*
/// `SitePresencePlugin`, and it caught this. Its own comment predicted the failure mode word for
/// word: *"a missing-`Res` panic is parameter validation, which fires the first time each system
/// actually runs"*. One panic then poisoned the `serial_guard` mutex and took four more tests with it.
///
/// The rule this restores is already written down in `input::claim_bindings` and `ui::mod`:
/// **the plugin that registers a reader claims the resource.** `init_resource` is idempotent and
/// never overwrites an inserted value, so claiming here cannot fight `SitePresencePlugin`.
pub fn claim_current_area(app: &mut App) {
    app.init_resource::<CurrentArea>();
    app.add_message::<AreaEntered>();
}

/// **Should this room's panel be up, and is it not?** — the spawn half of an auto-opening panel.
///
/// Keyed on the panel's own root component rather than on a message, so the answer is a fact about
/// the world instead of a fact about event history. A message-driven spawner has to be right about
/// every path into the room — walking in, entering the Site already standing in it (the five
/// operatives spawn in the briefing room), or a panel that failed to spawn because the HUD frame was
/// not up yet — and a missed message leaves a room permanently silent with nothing in the log.
pub fn panel_wanted<R: Component>(
    area: AreaId,
) -> impl Fn(Res<CurrentArea>, Query<(), With<R>>) -> bool + Clone {
    move |current: Res<CurrentArea>, panels: Query<(), With<R>>| {
        current.0 == Some(area) && panels.is_empty()
    }
}

/// **Is this room's panel up while the player is somewhere else?** — the despawn half.
pub fn panel_stale<R: Component>(
    area: AreaId,
) -> impl Fn(Res<CurrentArea>, Query<(), With<R>>) -> bool + Clone {
    move |current: Res<CurrentArea>, panels: Query<(), With<R>>| {
        current.0 != Some(area) && !panels.is_empty()
    }
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

    /// **The room ↔ region ledger.** Two panels in one room must not claim one region.
    ///
    /// `ui::layout:9` records that regions are claimed per-screen and collide **silently** — the
    /// second claimant simply does not appear. That was survivable while every hub panel spawned on
    /// `OnEnter(AppState::Site)` and the four claims were fixed for the whole screen. Auto-opening
    /// makes the claims *transient*, and the research wing deliberately hosts two panels at once
    /// (the curriculum and the experiments), so the invariant now has to be stated.
    ///
    /// ⚠️ This is a hand-kept ledger: it is a copy of what the four plugins register, not a read of
    /// it. Adding a room panel means adding a row. The thing it catches is the silent one.
    #[test]
    fn no_two_panels_in_one_room_claim_the_same_hud_region() {
        use crate::ui::layout::Region;
        let panels: &[(&str, Option<AreaId>, Region)] = &[
            ("records", Some(AreaId::Records), Region::BottomRight),
            ("requisition", Some(AreaId::Requisition), Region::BottomLeft),
            ("research experiments", Some(AreaId::Research), Region::TopRight),
            ("thaumiel curriculum", Some(AreaId::Research), Region::TopLeft),
            ("paratherapy", Some(AreaId::Activities), Region::BottomLeft),
            ("war room", Some(AreaId::WarRoom), Region::MidLeft),
            ("room name", None, Region::TopCenter),
            // Not room-gated: the teaching line follows the player everywhere in the hub, which is
            // the point of it. Listed so it is weighed against every room rather than forgotten.
            ("hint", None, Region::MidCenter),
        ];
        for (i, (name_a, area_a, region_a)) in panels.iter().enumerate() {
            for (name_b, area_b, region_b) in panels.iter().skip(i + 1) {
                // Two panels can share a region only if they can never be on screen together.
                let co_visible = match (area_a, area_b) {
                    (Some(a), Some(b)) => a == b,
                    _ => true, // an ungated panel is visible in every room
                };
                assert!(
                    !(co_visible && region_a == region_b),
                    "{name_a} and {name_b} are both up in {area_a:?}/{area_b:?} and both claim \
                     {region_a:?} — `layout::panel_in` gives it to whichever spawns first and the \
                     other vanishes with nothing in the log"
                );
            }
        }
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

    /// A marker standing in for a panel root.
    #[derive(Component)]
    struct FakePanel;

    /// **A room-gated panel must not panic in an app that lacks `SitePresencePlugin`.**
    ///
    /// This is the regression test for the defect `tests/replay.rs` caught on 2026-08-02. Putting
    /// `panel_wanted` on `RecordsPlugin`, `O5Plugin`, `BriefingPlugin` and friends gave every one of
    /// them a hard dependency on a resource only `SitePresencePlugin` inserts — and in Bevy 0.19 a
    /// missing `Res` in a run condition **panics on parameter validation** rather than skipping.
    ///
    /// Invisible in the shipped game, where `lib::run` registers them together. Very visible to a test
    /// that builds a subset, which is what the windowed-plugin-set replay test does — and one panic
    /// there poisoned the `serial_guard` mutex and took four more tests down with it.
    ///
    /// Lives here, in the fast GPU-free gate, so the next person finds out in a second rather than in
    /// a 53-minute harness run.
    #[test]
    fn a_room_gate_is_safe_in_an_app_that_never_registered_the_presence_plugin() {
        let mut app = App::new();
        // Exactly what a consumer plugin does, and nothing else — no `SitePresencePlugin`.
        claim_current_area(&mut app);
        app.init_resource::<Ran>().add_systems(
            Update,
            (|mut r: ResMut<Ran>| r.0 += 1).run_if(panel_wanted::<FakePanel>(AreaId::Records)),
        );
        app.update(); // would panic on `Res<CurrentArea>` without the claim
        assert_eq!(
            app.world().resource::<Ran>().0,
            0,
            "an unclaimed default is 'nowhere', so the panel stays shut — the claim must not \
             invent a room"
        );
    }

    /// The panel spawns once on entry and is torn down once on leaving — never respawned per frame.
    ///
    /// This is the failure the level-vs-edge distinction exists to prevent: a spawner keyed on "am I
    /// in the room" rather than "am I in the room AND is the panel missing" rebuilds its whole
    /// subtree sixty times a second, and `review.rs:186` records what that costs — the button dies
    /// under the cursor mid-click.
    #[test]
    fn a_room_panel_spawns_once_and_despawns_once() {
        let mut app = App::new();
        app.insert_resource(CurrentArea(None))
            .init_resource::<Ran>()
            .add_systems(
                Update,
                (
                    (|mut c: Commands, mut r: ResMut<Ran>| {
                        c.spawn(FakePanel);
                        r.0 += 1;
                    })
                    .run_if(panel_wanted::<FakePanel>(AreaId::Records)),
                    (|mut c: Commands, q: Query<Entity, With<FakePanel>>| {
                        for e in &q {
                            c.entity(e).despawn();
                        }
                    })
                    .run_if(panel_stale::<FakePanel>(AreaId::Records)),
                )
                    .chain(),
            );

        app.insert_resource(CurrentArea(Some(AreaId::Records)));
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<Ran>().0,
            1,
            "five frames in the same room must spawn the panel ONCE"
        );
        assert_eq!(
            app.world_mut().query::<&FakePanel>().iter(app.world()).count(),
            1
        );

        app.insert_resource(CurrentArea(Some(AreaId::Kitchen)));
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            app.world_mut().query::<&FakePanel>().iter(app.world()).count(),
            0,
            "walking out closes it"
        );
        assert_eq!(
            app.world().resource::<Ran>().0,
            1,
            "and leaving must not have spawned it again on the way"
        );

        // ...and walking back in opens it again. The resource is the state, so this needs no memory.
        app.insert_resource(CurrentArea(Some(AreaId::Records)));
        app.update();
        assert_eq!(app.world().resource::<Ran>().0, 2);
    }
}
