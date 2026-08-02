//! **The rooms say their names.**
//!
//! # The label that was authored twelve times and shown zero
//!
//! `layout::Area::label` carries player-facing copy — `ASYNC APERTURE`, `CONTAINMENT`, `RECORDS`,
//! `GALLEY`, `WAR ROOM` — and its own doc comment says *"shown on the floor decal / signage"*. Every
//! one of the twelve areas authors one. Grepping every consumer in `src/` and `tests/` on 2026-08-02
//! turned up exactly two: both inside `warn!` strings in `check_prop_placements`, describing a prop
//! that stuck out of a room. **No player has ever seen one.**
//!
//! What the hub navigates by instead is `SitePiece::AreaDecal`, the untextured wing colour-code
//! `pieces.rs` describes as *"what makes the hub learnable without signage"*. That is a good system
//! and this does not replace it — a colour tells you where you are at a glance and from across the
//! building, which a word cannot. What a colour cannot do is tell you the room's **name**, and the
//! Site talks about its rooms by name everywhere else: the design doc, the staff roster, the
//! conversations, this module's own siblings.
//!
//! # Why a readout and not a sign on the wall
//!
//! A physical placard was the first design and it is the wrong first step, for a reason the layout
//! records: **the Site's rooms have no doors.** An opening is the *absence* of wall, so a room can be
//! open along a whole side, and "the wall by the entrance" is not a thing every room has — the same
//! trap that made 31 placement faults out of furniture authored against a north wall that did not
//! exist. A sign also faces one way, so at two of the four camera detents it is edge-on or behind you.
//!
//! So the name goes where the player is already looking, and it behaves like a sign rather than like
//! a HUD element: it appears when you cross into a room and **fades**, because a permanent label is
//! HUD budget spent forever on someone who learned the building in their first visit. That is
//! `ui::hint`'s rule and its citation — Cockburn, Gutwin, Scarr & Malacria 2014, *Supporting Novice
//! to Expert Transitions in User Interfaces* (ACM Comput. Surv. 47(2), DOI 10.1145/2659796) — whose
//! finding is that a persistent aid keeps users on the method they learned first.
//!
//! Windowed-only, `Update`, and it writes nothing but its own `Text` and `TextColor`.

use bevy::prelude::*;

use super::presence::{AreaEntered, CurrentArea};
use super::visuals::SiteLayoutRes;
use crate::ui::layout::{self, HudRegions, Region};
use crate::ui::state::{despawn_scoped, AppState};
use crate::ui::theme::{FontAssets, UiTheme};

/// Seconds the name holds at full strength before it starts to go.
const HOLD: f32 = 1.6;
/// Seconds it takes to fade out once it starts.
const FADE: f32 = 1.1;

/// Root of the room-name readout.
#[derive(Component)]
pub struct SignageRoot;

/// The single line, and how long it has been up.
#[derive(Component)]
pub struct RoomName {
    /// Seconds since the player entered this room. Real time, so it behaves the same at any game
    /// speed and while a conversation has the sim frozen — the same clock `ui::hint` and the camera
    /// ease use, and for the same reason.
    pub age: f32,
}

/// Where the name sits. `TopCenter` is free in every room — the four room panels claim the corners
/// and the teaching hint claims `MidCenter` — which is what
/// `presence::no_two_panels_in_one_room_claim_the_same_hud_region` exists to keep true.
const REGION: Region = Region::TopCenter;

fn spawn_signage(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    regions: Res<HudRegions>,
) {
    let root = (
        SignageRoot,
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
    );
    let Some(mut ec) = layout::panel_in(&mut commands, &regions, REGION, root) else {
        error!("site signage: no layout frame at spawn — the rooms will not name themselves");
        return;
    };
    ec.with_children(|p| {
        p.spawn((
            RoomName { age: f32::MAX }, // starts expired: no name until the player enters a room
            crate::ui::widgets::text_colored(&theme, &fonts, "", theme.font_body, theme.text),
            Pickable::IGNORE,
        ));
    });
}

/// **The alpha a name is drawn at, `age` seconds after entering the room.**
///
/// Pure, so the whole retire-and-fade behaviour is testable without an `App` — including the part
/// that matters, which is that it genuinely reaches zero rather than approaching it. A label easing
/// asymptotically toward transparent is a label that never leaves, and it would keep the text node
/// re-rendering for the rest of the session.
pub fn name_alpha(age: f32) -> f32 {
    if age <= HOLD {
        return 1.0;
    }
    let t = (age - HOLD) / FADE;
    (1.0 - t).clamp(0.0, 1.0)
}

/// Re-arm the name when the player crosses into a room, and fade it out again.
///
/// Reads [`AreaEntered`] rather than diffing [`CurrentArea`] itself, which is the whole reason that
/// message exists: the edge is published once, by one writer, and every reader agrees about when it
/// happened.
pub fn update_signage(
    time: Res<Time<bevy::time::Real>>,
    theme: Res<UiTheme>,
    layout: Option<Res<SiteLayoutRes>>,
    current: Res<CurrentArea>,
    mut entered: MessageReader<AreaEntered>,
    mut names: Query<(&mut RoomName, &mut Text, &mut TextColor)>,
) {
    let Some(layout) = layout else { return };
    // Drained even when there is no readout to write, so a name cannot be delivered late to a panel
    // that spawns a frame after the player walked in.
    let crossed = entered.read().count() > 0;

    for (mut name, mut text, mut color) in &mut names {
        if crossed {
            name.age = 0.0;
            // The area's OWN authored copy, never a `{:?}` of the enum: `Kitchen` is the variant and
            // `GALLEY` is what the Foundation calls the room. Printing the identifier would leak the
            // programmer's word for it into the fiction.
            let want = current
                .0
                .and_then(|id| layout.0.area(id))
                .map(|a| a.label.clone())
                .unwrap_or_default();
            if text.0 != want {
                text.0 = want;
            }
        } else {
            name.age += time.delta_secs();
        }
        let alpha = name_alpha(name.age);
        let want = theme.text.with_alpha(alpha);
        if color.0 != want {
            color.0 = want;
        }
    }
}

pub struct SiteSignagePlugin;

impl Plugin for SiteSignagePlugin {
    fn build(&self, app: &mut App) {
        crate::site::claim_current_area(app);
        app.add_systems(
            OnEnter(AppState::Site),
            spawn_signage.after(layout::spawn_frame),
        )
        .add_systems(OnExit(AppState::Site), despawn_scoped::<SignageRoot>)
        .add_systems(Update, update_signage.run_if(in_state(AppState::Site)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name is legible, then it is gone — and *gone*, not merely faint.
    #[test]
    fn a_room_name_holds_then_retires_completely() {
        assert_eq!(name_alpha(0.0), 1.0, "readable the instant you walk in");
        assert_eq!(name_alpha(HOLD), 1.0, "and for long enough to read");
        let mid = name_alpha(HOLD + FADE * 0.5);
        assert!(mid > 0.0 && mid < 1.0, "it fades rather than blinking out: {mid}");
        assert_eq!(
            name_alpha(HOLD + FADE),
            0.0,
            "an asymptotic fade is a label that never leaves — HUD budget spent forever on \
             somebody who learned the building on their first visit (Cockburn et al. 2014)"
        );
        assert_eq!(name_alpha(f32::MAX), 0.0, "and it stays gone");
    }

    /// **Every area shows its authored copy, and none of them shows an enum name.**
    ///
    /// The specific slip this guards is `Kitchen`, whose player-facing name is `GALLEY`. A readout
    /// built from `{:?}` would be correct-looking, would pass a smoke test, and would put the
    /// programmer's word for the room into the fiction.
    #[test]
    fn every_room_has_player_facing_copy_and_it_is_not_the_variant_name() {
        let l = crate::site::SiteLayout::load().expect("the shipped layout must load");
        for a in &l.areas {
            assert!(!a.label.trim().is_empty(), "{:?} has no name to show", a.id);
            assert_eq!(
                a.label,
                a.label.to_uppercase(),
                "{:?}'s label is player-facing copy in the Foundation's voice, which this HUD \
                 renders verbatim — it is set in caps at the source, not upcased at the door",
                a.id
            );
        }
        let kitchen = l.area(crate::site::AreaId::Kitchen).expect("the galley exists");
        assert_eq!(
            kitchen.label, "GALLEY",
            "the one that catches a `{{:?}}` readout — the variant is Kitchen and the room is the \
             galley"
        );
    }
}
