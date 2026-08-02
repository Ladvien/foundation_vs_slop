//! **The expedition briefing** (FVS-L-4) — which Branch universe you are being sent into, and what the
//! director expects of it.
//!
//! # Why the director needs a face at all
//!
//! FVS-H-3 samples a world from the QD archive by where the player is learning fastest. Without this
//! panel that is *completely invisible*: the worlds change, the reason does not surface, and adaptive
//! difficulty is indistinguishable from randomness — or worse, from the game being inconsistent. A
//! director the player cannot perceive is a director that gets blamed for bad luck.
//!
//! # It describes the world in the archive's OWN axes
//!
//! `clutter` and `infestation` are the level archive's behaviour descriptors. The briefing renders those
//! rather than collapsing them into a single "difficulty: 7/10", and that is deliberate: MAP-Elites does
//! not produce a difficulty scalar, and inventing one here would throw away exactly the thing the
//! archive exists to preserve — that two worlds can be equally hard in *different ways*. A dense, clean
//! ruin and an open, infested one are not interchangeable, and the player can act on the difference.
//!
//! # The authored-world case is stated, not hidden (FVS-H-7)
//!
//! With no archive to sample, `config.ron`'s authored world plays. `director` argues that is one path
//! rather than a fallback — the authored world is the *right* expedition, not a degraded substitute —
//! but that argument is the shape a fallback uses to justify itself. The honest test is whether the
//! player can tell, so the briefing says `AUTHORED UNIVERSE — NO ARCHIVE SAMPLED` in as many words. A
//! path you cannot perceive is a second path however the code frames it.
//!
//! Windowed-only: `OnEnter`/`OnExit` + `Update`, reads state and writes nothing the sim reads.

use bevy::prelude::*;

use super::layout::{self, HudRegions, Region};
use super::state::{despawn_scoped, AppState};
use super::theme::{FontAssets, UiTheme};
use crate::director::ExpeditionBriefing;

/// Root marker, so the panel is despawned with the rest of the in-game UI.
#[derive(Component)]
pub struct BriefingRoot;

/// A five-block bar for a `[0, 1]` descriptor.
///
/// Blocks rather than a number because the axes are *comparative* — what matters is that this world is
/// more cluttered than the last one, not that it scores 0.62.
fn meter(v: f32) -> String {
    let filled = (v.clamp(0.0, 1.0) * 5.0).round() as usize;
    format!("{}{}", "▓".repeat(filled), "░".repeat(5 - filled))
}

/// The briefing text. Pure, so the wording is testable without an `App`.
pub fn briefing_text(b: &ExpeditionBriefing) -> String {
    match b.0 {
        None => {
            // FVS-H-7's tell. Named as a *universe*, in the same voice as the sampled case, so it reads
            // as a legitimate expedition rather than as an error the player should worry about.
            "EXPEDITION BRIEFING\n\
             AUTHORED UNIVERSE — NO ARCHIVE SAMPLED\n\
             Baseline site conditions. Nothing has been tuned to you."
                .to_string()
        }
        Some(b) => {
            let c = b.challenge;
            format!(
                "EXPEDITION BRIEFING\n\
                 BRANCH UNIVERSE {:#X}   ·   SECTOR {},{}\n\
                 CLUTTER      {}\n\
                 INFESTATION  {}",
                b.seed,
                c.cell.0,
                c.cell.1,
                meter(c.clutter),
                meter(c.infestation),
            )
        }
    }
}

fn spawn_briefing(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    regions: Res<HudRegions>,
    briefing: Res<ExpeditionBriefing>,
) {
    let root = (
        BriefingRoot,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_sm)),
            ..default()
        },
        BackgroundColor(theme.panel),
        super::widgets::border_all(theme.panel_border),
        Pickable::IGNORE,
    );
    let Some(mut ec) = layout::panel_in(&mut commands, &regions, Region::TopLeft, root) else {
        error!("briefing: no layout frame at spawn — the expedition framing is not shown");
        return;
    };
    ec.with_children(|p| {
            // Through the shared widget rather than a hand-rolled bundle: `widgets::text_colored`
            // emits `FontSize::Rem`, which is what makes the accessibility text-scale reach this
            // panel. A hand-rolled `FontSize::Px` bundle would stop being readable for exactly the
            // players who turned the scale up.
            p.spawn(super::widgets::text_colored(
                &theme,
                &fonts,
                briefing_text(&briefing),
                theme.font_body,
                theme.text_muted,
            ));
        });
}

/// Root marker for the **war room's** copy of the briefing, at Site-67.
#[derive(Component)]
pub struct WarRoomRoot;

/// What the war room says about the last expedition.
///
/// The same descriptors, re-headed. The tense is the whole difference and it is not cosmetic: at the
/// Site the director has not sampled the *next* world yet, so a panel headed `EXPEDITION BRIEFING`
/// would be claiming to describe a world that does not exist. `ExpeditionBriefing` while `Idle` holds
/// the one you came back from, and saying so is the same honesty rule FVS-H-7 sets for the
/// authored-world case — a state the player cannot perceive is a second path however the code frames
/// it.
pub fn war_room_text(b: &ExpeditionBriefing) -> String {
    match b.0 {
        // Before the first expedition there is nothing to review, and the room should say so rather
        // than render an authored-world briefing that reads like a plan (`docs/ui.md` §1.4).
        None => "WAR ROOM\n\
                 NO EXPEDITION ON RECORD.\n\
                 The board is clear until you have been through the door."
            .to_string(),
        Some(_) => briefing_text(b).replacen(
            "EXPEDITION BRIEFING",
            "WAR ROOM — LAST EXPEDITION",
            1,
        ),
    }
}

fn spawn_war_room(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    regions: Res<HudRegions>,
    briefing: Res<ExpeditionBriefing>,
) {
    let root = (
        WarRoomRoot,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_sm)),
            ..default()
        },
        BackgroundColor(theme.panel),
        crate::ui::widgets::border_all(theme.panel_border),
        Pickable::IGNORE,
    );
    // `MidLeft` — free in every room. The four corner panels and the war room can never be on screen
    // together anyway, but taking a distinct region means the ledger test in `site::presence` stays
    // provable rather than depending on the gating being right.
    let Some(mut ec) = layout::panel_in(&mut commands, &regions, Region::MidLeft, root) else {
        error!("war room: no layout frame at spawn — the branch readout is not shown");
        return;
    };
    ec.with_children(|p| {
        p.spawn((
            crate::ui::widgets::text_colored(
                &theme,
                &fonts,
                war_room_text(&briefing),
                theme.font_body,
                theme.text,
            ),
            Pickable::IGNORE,
        ));
    });
}

pub struct BriefingPlugin;

impl Plugin for BriefingPlugin {
    fn build(&self, app: &mut App) {
        // ── The war room, at the Site ────────────────────────────────────────────────────────────
        //
        // `ui/briefing` has rendered `BRANCH UNIVERSE 0x… CLUTTER ▓▓▓░░ INFESTATION ▓▓░░░` since
        // FVS-L-4, and the room named after it did not show it. Now it does, when you stand in it.
        //
        // ⚠️ **`RunState::Idle` only, and that is a rule rather than an optimisation.**
        // `docs/2026-08-01-two-live-layers.md` §5 forbids supervising the squad you left unattended:
        // during a `VisitSite` the expedition is still running and reading its board would be exactly
        // the surveillance that document rules out. The room is dark during a visit, deliberately, and
        // `AreaId::WarRoom`'s own doc comment says so.
        app.add_systems(
            Update,
            spawn_war_room
                .after(layout::spawn_frame)
                .run_if(in_state(crate::session::RunState::Idle))
                .run_if(crate::site::panel_wanted::<WarRoomRoot>(
                    crate::site::AreaId::WarRoom,
                )),
        )
        .add_systems(
            Update,
            despawn_scoped::<WarRoomRoot>.run_if(
                crate::site::panel_stale::<WarRoomRoot>(crate::site::AreaId::WarRoom)
                    .or_else(in_state(crate::session::RunState::Active)),
            ),
        )
        .add_systems(OnExit(AppState::Site), despawn_scoped::<WarRoomRoot>);
        // `OnEnter(InGame)` rather than `OnEnter(Warmup)`: the director writes the briefing during
        // `OnEnter(RunState::Active)`, which lands before `Warmup` finishes, but reading it at `InGame`
        // means the panel cannot render a briefing for the *previous* expedition if a transition is
        // ever reordered.
        app.add_systems(
            OnEnter(AppState::InGame),
            spawn_briefing.after(layout::spawn_frame),
        )
        .add_systems(OnExit(AppState::InGame), despawn_scoped::<BriefingRoot>);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::director::Briefing;
    use crate::elite_overlay::LevelChallenge;

    fn sampled(clutter: f32, infestation: f32) -> ExpeditionBriefing {
        ExpeditionBriefing(Some(Briefing {
            challenge: LevelChallenge { cell: (3, 5), clutter, infestation, fitness: 0.8 },
            seed: 0x5C09191,
        }))
    }

    #[test]
    fn a_sampled_world_names_its_universe_and_both_axes() {
        let t = briefing_text(&sampled(0.8, 0.2));
        assert!(t.contains("0x5C09191"), "the Branch universe must be identified: {t}");
        assert!(t.contains("3,5"), "the archive cell must be identified: {t}");
        assert!(t.contains("CLUTTER") && t.contains("INFESTATION"), "{t}");
    }

    /// **The war room reviews; it does not plan.**
    ///
    /// At the Site the director has not sampled the next world yet, so a panel headed `EXPEDITION
    /// BRIEFING` would be describing a world that does not exist. The tense is the whole difference
    /// between an honest readout and one the player would act on wrongly.
    #[test]
    fn the_war_room_speaks_in_the_past_tense_and_keeps_both_axes() {
        let t = war_room_text(&sampled(0.8, 0.2));
        assert!(t.contains("LAST EXPEDITION"), "the tense is the point: {t}");
        assert!(
            !t.contains("EXPEDITION BRIEFING"),
            "it must not claim to brief a world nobody has sampled: {t}"
        );
        // ...and it still carries what the briefing carries, in the archive's own axes.
        assert!(t.contains("0x5C09191") && t.contains("CLUTTER") && t.contains("INFESTATION"), "{t}");
    }

    /// Before the first expedition the board is empty, and the room says so.
    ///
    /// Falling through to the authored-world briefing would print "Baseline site conditions" under a
    /// war-room header, which reads as a plan for an expedition that has not been prepared.
    #[test]
    fn an_empty_board_is_named_rather_than_filled_with_the_authored_world() {
        let t = war_room_text(&ExpeditionBriefing(None));
        assert!(t.contains("NO EXPEDITION ON RECORD"), "{t}");
        assert!(!t.contains("AUTHORED UNIVERSE"), "that is the in-game briefing's line, not this one: {t}");
    }

    #[test]
    fn the_axes_are_shown_separately_not_collapsed_into_a_difficulty_score() {
        // THE property this panel exists to preserve. MAP-Elites does not produce a difficulty scalar,
        // and two worlds can be equally hard in different ways — a dense clean ruin and an open
        // infested one are not interchangeable. Collapsing them would discard what the archive is for.
        let cluttered = briefing_text(&sampled(0.9, 0.1));
        let infested = briefing_text(&sampled(0.1, 0.9));
        assert_ne!(cluttered, infested, "two differently-shaped worlds must not brief identically");
    }

    #[test]
    fn the_authored_world_says_so_in_as_many_words() {
        // FVS-H-7. `director` claims "no archive -> the authored world" is one path rather than a
        // fallback; the honest test is whether the player can TELL. If this ever goes silent, the
        // campaign is alternating between two worlds' provenance with no way to perceive which.
        let t = briefing_text(&ExpeditionBriefing::default());
        assert!(t.contains("AUTHORED UNIVERSE"), "the unsampled case must be legible: {t}");
        assert!(t.contains("BRIEFING"), "and must still read as a briefing, not an error: {t}");
    }

    #[test]
    fn the_meter_spans_its_range_without_overflowing() {
        assert_eq!(meter(0.0).chars().count(), 5);
        assert_eq!(meter(1.0).chars().count(), 5);
        // Out-of-range input is clamped rather than panicking on a negative repeat count — the archive
        // is data on disk and a corrupt descriptor must not take the UI down.
        assert_eq!(meter(-3.0).chars().count(), 5);
        assert_eq!(meter(9.0).chars().count(), 5);
        assert_ne!(meter(0.0), meter(1.0));
    }
}
