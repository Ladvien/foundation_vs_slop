//! **Teaching lines** — the one-off prompts that tell the player a verb exists.
//!
//! This module exists because of a specific, embarrassing observation: the squad's own authored
//! dialogue asks *"How do we play this?"*, and for the two verbs that move the player between the
//! expedition and Site-67 the game had no answer. `Action::VisitSite` was reachable only from the
//! controls screen, and the ASYNC door — the sole route in or out of the hub before the Tab toggle —
//! was taught nowhere at all: `ui::site_hud` spawns a curriculum panel and a button, and neither
//! mentions it.
//!
//! ## Three rules it follows
//!
//! **It reads the live binding, never a letter.** The text is built from
//! [`KeyBindings::key_label`], so a player who rebinds is told the key they actually have.
//! `key_label` and **not** `key_char`: `key_char` returns `'?'` for anything that is not a single
//! glyph, and the default binding here is `Tab`. Telling the player a key that does nothing is worse
//! than telling them none (`input::KeyBindings::key_char` records why that rule exists).
//!
//! **It retires.** Cockburn, Gutwin, Scarr & Malacria 2014 (*Supporting Novice to Expert Transitions
//! in User Interfaces*, ACM Comput. Surv. 47(2), DOI 10.1145/2659796) — already cited by
//! `crate::selection` — document the intermodal-transition failure: users plateau on the method they
//! learned first and never move on. A hint that never leaves is HUD budget spent forever on someone
//! who learned the key in the first thirty seconds, and `docs/ui.md` §2 is explicit that what a lower
//! density sheds first is what competes for attention. So the visit hints are gone for good once the
//! player has been to the Site once, remembered in `settings::OnboardingSettings`.
//!
//! **It never claims a key does something it does not.** The `Tab` return line only appears while an
//! expedition is actually live. Standing in the hub between runs, `Tab` is inert — there is nothing
//! to return *to* — so the prompt there is the aperture one instead, and that one does **not** retire,
//! because walking through the door is still the only way to *begin* an expedition.

use bevy::prelude::*;

use super::layout::{self, HudRegions, Region};
use super::state::{despawn_scoped, AppState};
use super::theme::{FontAssets, UiTheme};
use super::widgets::text_colored;
use crate::input::{Action, KeyBindings};
use crate::settings::OnboardingSettings;

/// Root of the hint panel, so it can be despawned wholesale on leaving either screen.
#[derive(Component)]
pub struct HintRoot;

/// The single line of text the panel owns.
#[derive(Component)]
pub struct HintLine;

pub struct HintPlugin;

impl Plugin for HintPlugin {
    fn build(&self, app: &mut App) {
        // The plugin that registers a reader is what guarantees the resource exists — the contract
        // `camera` and `site::visuals` both state.
        crate::input::claim_bindings(app);
        // `SettingsPlugin` inserts the real, disk-loaded value; this only guarantees the resource is
        // present for the bare-`App` UI-liveness test, exactly as `DebugCaptureActive` is claimed in
        // `ui::UiPlugin`. `init_resource` is idempotent and never overwrites an inserted value.
        app.init_resource::<OnboardingSettings>()
            // Both screens host the panel; `HudLayoutPlugin` spawns a frame for each, and every panel
            // must order itself after that or it silently spawns nothing.
            .add_systems(OnEnter(AppState::InGame), spawn_panel.after(layout::spawn_frame))
            .add_systems(OnEnter(AppState::Site), spawn_panel.after(layout::spawn_frame))
            .add_systems(OnExit(AppState::InGame), despawn_scoped::<HintRoot>)
            .add_systems(OnExit(AppState::Site), despawn_scoped::<HintRoot>)
            .add_systems(
                Update,
                update_hint.run_if(in_state(AppState::InGame).or(in_state(AppState::Site))),
            );
    }
}

fn spawn_panel(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    regions: Res<HudRegions>,
) {
    let root = (
        HintRoot,
        Node { flex_direction: FlexDirection::Column, ..default() },
        // No background and no border: this is a line of text, not a panel. An empty hint therefore
        // renders nothing at all rather than an empty box hanging in the middle of the screen.
        Pickable::IGNORE,
    );
    // `MidCenter` is the one region no other panel claims in either state.
    let Some(mut ec) = layout::panel_in(&mut commands, &regions, Region::MidCenter, root) else {
        error!("hint: no layout frame at spawn — the controls prompt is not shown");
        return;
    };
    ec.with_children(|p| {
        p.spawn((
            HintLine,
            // Muted and slightly smaller — the same treatment `verb_bar`'s hover hint gets, and for
            // the same stated reason: it is teaching rather than reporting, so it must not compete.
            text_colored(&theme, &fonts, "", theme.font_body * 0.85, theme.text_muted),
            Pickable::IGNORE,
        ));
    });
}

/// What the player should be told right now, or `None` for "nothing" — split out as a pure function
/// so every branch is unit-testable without an `App`, including the one that must never fire (a `Tab`
/// prompt at the Site with no expedition running).
fn hint_text(
    app_state: &AppState,
    run_active: bool,
    key: &str,
    ob: &OnboardingSettings,
) -> Option<String> {
    match app_state {
        // Mid-expedition: the verb the player has no other way to discover.
        AppState::InGame if run_active && !ob.learned_visit => {
            Some(format!("{key}  —  SITE-67      the squad keeps its orders while you are away"))
        }
        // At the hub with a live expedition: this is a *visit*, and it has a way home. Keyed on its
        // OWN flag — sharing one with the hint above would retire this before it could ever be read,
        // because reaching the Site is what would have set it.
        AppState::Site if run_active && !ob.learned_return => {
            Some(format!("{key}  —  back to the expedition"))
        }
        // At the hub between expeditions. `Tab` does nothing here, so it is deliberately not
        // mentioned; the door is the only verb, and it never retires because it is the only way in.
        AppState::Site if !run_active => {
            Some("Walk an operative into the ASYNC aperture to begin an expedition".to_string())
        }
        _ => None,
    }
}

fn update_hint(
    app_state: Res<State<AppState>>,
    // Optional: the bare-`App` UI-liveness test adds `UiPlugin` without `SessionPlugin`, so the run
    // state genuinely may not exist. Absent means no session at all, which is exactly "no expedition
    // to go back to" — the same answer `Idle` gives.
    run_state: Option<Res<State<crate::session::RunState>>>,
    bindings: Res<KeyBindings>,
    onboarding: Res<OnboardingSettings>,
    mut lines: Query<&mut Text, With<HintLine>>,
) {
    let run_active =
        run_state.is_some_and(|s| *s.get() == crate::session::RunState::Active);
    let key = bindings.key_label(Action::VisitSite);
    let want = hint_text(app_state.get(), run_active, &key, &onboarding).unwrap_or_default();
    for mut text in &mut lines {
        if text.0 != want {
            text.0 = want.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A profile that has learned neither verb.
    fn fresh() -> OnboardingSettings {
        OnboardingSettings::default()
    }

    #[test]
    fn the_visit_hint_names_the_live_key_and_then_retires() {
        let first = hint_text(&AppState::InGame, true, "Tab", &fresh()).expect("a new player is taught");
        assert!(first.starts_with("Tab"), "the hint must lead with the key: {first}");
        assert!(first.contains("SITE-67"), "and name where it takes you: {first}");
        let learned = OnboardingSettings { learned_visit: true, ..fresh() };
        assert_eq!(
            hint_text(&AppState::InGame, true, "Tab", &learned),
            None,
            "a player who has used the verb must not be told again"
        );
    }

    /// **The bug this shape exists to prevent.** With one shared flag, arriving at the Site retired the
    /// hint that tells you how to LEAVE it — in the same transition — so it could never be read by
    /// anyone. Learning the outbound verb must leave the inbound hint standing.
    #[test]
    fn learning_the_way_out_does_not_retire_the_way_back() {
        let out_only = OnboardingSettings { learned_visit: true, learned_return: false };
        let back = hint_text(&AppState::Site, true, "Tab", &out_only)
            .expect("the return hint must survive learning the outbound verb");
        assert!(back.contains("back to the expedition"), "{back}");
        // And once the return has been used too, both are done.
        let both = OnboardingSettings { learned_visit: true, learned_return: true };
        assert_eq!(hint_text(&AppState::Site, true, "Tab", &both), None);
    }

    /// **The branch that would be a lie.** `Tab` only returns you to an expedition that exists; at the
    /// Site between runs it does nothing, so the hint must not offer it.
    #[test]
    fn the_site_never_offers_tab_without_a_live_expedition() {
        let idle = hint_text(&AppState::Site, false, "Tab", &fresh()).expect("the door is always taught");
        assert!(!idle.contains("Tab"), "Tab does nothing here and must not be offered: {idle}");
        assert!(idle.contains("aperture"), "the only verb here is the door: {idle}");
    }

    /// The aperture prompt is the one that does **not** retire — it is how an expedition begins.
    #[test]
    fn the_door_prompt_outlives_onboarding() {
        let both = OnboardingSettings { learned_visit: true, learned_return: true };
        assert!(
            hint_text(&AppState::Site, false, "Tab", &both).is_some(),
            "walking through the door is still the only way to start a run"
        );
    }

    /// Rebinding must move the hint with it — the whole reason this reads `key_label` at runtime.
    #[test]
    fn the_hint_follows_a_rebind() {
        let rebound = hint_text(&AppState::InGame, true, "F8", &fresh()).expect("still taught");
        assert!(rebound.starts_with("F8"), "the hint must show the CURRENT key: {rebound}");
    }

    /// Screens that are not the two live ones say nothing.
    #[test]
    fn other_screens_are_silent() {
        for state in [AppState::Boot, AppState::Title, AppState::Warmup, AppState::Debrief] {
            assert_eq!(hint_text(&state, true, "Tab", &fresh()), None, "{state:?} must be silent");
        }
    }
}
