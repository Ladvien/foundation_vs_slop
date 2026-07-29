//! **The event line** — one transient row for the thing that just happened.
//!
//! # The gap
//!
//! This game had **no log, toast, killfeed or notification of any kind**. Every readout was a
//! *standing* state: health now, containment now, the objective now. Nothing reported an *event* —
//! an operative going down, an anomaly breaking containment, a nest capped. The player learned those
//! by watching the world and noticing a number change, which works for the thing they are looking at
//! and not at all for the thing they are not.
//!
//! # Why one line and not a scrolling log
//!
//! Ancker et al. 2017 (`10.1186/s12911-017-0430-8`, already `docs/ui.md` §3.4's source) measured
//! acceptance of clinical advisories dropping **~30% for each additional alert per encounter**, and a
//! further ~10% for every 5-percentage-point rise in the share of *repeated* alerts. Their finding on
//! the mechanism is the one that shapes this module: across six newly deployed alerts acceptance
//! showed **no decline over time**, so it is *not* desensitisation. It is cognitive overload from
//! uninformative volume, and an uninformative alert "is essentially a false alarm."
//!
//! So: **one line, one event, deduped by entity.** A scrolling log would be a volume dial, and the
//! evidence says you cannot fix alert spam by animating harder or recolouring — you delete the
//! low-informativeness alerts. [`Severity`] is the budget: only [`Severity::Beat`] events reach this
//! line at all, and a `Beat` fires at most once per entity per encounter.
//!
//! # The pip
//!
//! Every appearance is paired with a short audio tick. Van der Burg, Olivers, Bronkhorst & Theeuwes
//! 2008 (*Pip and pop: Nonspatial auditory signals improve spatial visual search*, JEP:HPP 34(5),
//! DOI 10.1037/0096-1523.34.5.1053) is the source, and the measured effect is much larger than the
//! "helps a bit" it is usually cited as. Experiment 1, searching a cluttered display of up to 48
//! continuously recolouring distractors:
//!
//! - search slope **147 ms/item without the tone → 31 ms/item with it**;
//! - tone presence F(1,5)=10.7, p<.05, ηp=.68; Tone × Set Size F(2,10)=12.7, p<.005, ηp=.72;
//! - **with the tone the set-size effect stopped being significant at all** (F(2,10)=1.9, p=.224) —
//!   the target went from being hunted to being seen.
//!
//! Two controls in the same paper are why this is the right instrument rather than a generic "add a
//! sound": it is **not general alerting**, because the effect "does not occur with visual cues"; and
//! it is **not top-down cuing**, because it survives synchronising the pip with *distractors* on most
//! trials. The tone carries no location, colour or identity information — only the *moment*. Their
//! account is that auditory temporal resolution exceeds visual, so the synchronous pair becomes a
//! salient emergent feature.
//!
//! That buys salience for **zero pixels**, which is exactly the currency a horror HUD wants to spend
//! in, and it doubles as atmosphere. It is also the reason the pip must fire *with* the line and not
//! near it: synchrony is the whole mechanism.
//!
//! # Where it sits, and why it fades
//!
//! MidLeft — one of the regions that were empty during play. It fades out rather than persisting:
//! Lewandowska, Dziśko & Jankowski 2022 (`10.1038/s41598-022-16284-2`) found a *constant*
//! high-contrast peripheral element habituates away and stops working when it matters, while
//! sustained high intensity "can cause unnecessary irritation or even cognitive load for more
//! extended usage." A line that arrives, is read, and leaves is a contrast **delta**; a permanent one
//! is a steady state that has stopped carrying information.
//!
//! Windowed-only, `Update` only, reads sim state and writes none — nothing here reaches
//! `snapshot_hash`.

use std::collections::HashSet;

use bevy::prelude::*;

use super::layout::{self, HudRegions, Region};
use super::state::{despawn_scoped, AppState};
use super::theme::{glyph, FontAssets, UiTheme};
use super::widgets::text_colored;

/// Seconds a line stays fully lit before it begins to fade.
const HOLD_SECS: f32 = 2.2;

/// Seconds the fade takes. Long enough to be a fade rather than a blink.
const FADE_SECS: f32 = 1.3;

/// How loud an event is. The **budget**, not a styling knob.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// A tactical beat: this is the class that reaches the line. One per entity per encounter.
    Beat,
    /// Everything else. Deliberately has no surface at all — it is not shown quieter, it is *not
    /// shown*. Ancker et al.'s finding is that low-informativeness alerts must be deleted, not
    /// demoted, because volume is the mechanism rather than intensity.
    Passive,
}

/// Something worth one line.
///
/// `subject` is what the event is *about*, and it is what dedupe keys on — so one operative going
/// down cannot report itself twice, however many systems notice.
#[derive(Message, Clone, Debug)]
pub struct GameEvent {
    pub severity: Severity,
    /// The entity the event concerns, for dedupe. `None` for events with no subject (phase changes).
    pub subject: Option<Entity>,
    /// The line, already phrased as a statement about the world.
    pub text: String,
    /// A status glyph — the redundant, non-colour channel (`docs/ui.md` §1.3).
    pub glyph: &'static str,
}

impl GameEvent {
    /// A tactical beat about a specific entity.
    pub fn beat(subject: Entity, text: impl Into<String>) -> Self {
        GameEvent {
            severity: Severity::Beat,
            subject: Some(subject),
            text: text.into(),
            glyph: glyph::UNMET,
        }
    }

    /// A beat about the run rather than an entity (a phase change).
    pub fn note(text: impl Into<String>) -> Self {
        GameEvent {
            severity: Severity::Beat,
            subject: None,
            text: text.into(),
            glyph: glyph::CURRENT,
        }
    }
}

/// The line's marker + its own fade clock.
#[derive(Component)]
pub struct EventLine {
    /// Seconds since the current line was posted. `None` when nothing is showing.
    since: Option<f32>,
}

/// Entities already reported this encounter, so one subject cannot fire twice.
///
/// Cleared on entering a run rather than on a timer: "per encounter" is the unit Ancker et al.
/// measured acceptance against, and an expedition is this game's encounter.
#[derive(Resource, Default)]
pub struct ReportedSubjects(HashSet<Entity>);

pub struct EventLinePlugin;

impl Plugin for EventLinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ReportedSubjects>()
            .add_message::<GameEvent>()
            .add_systems(
                OnEnter(crate::session::RunState::Active),
                |mut seen: ResMut<ReportedSubjects>| seen.0.clear(),
            )
            .add_systems(
                OnEnter(AppState::InGame),
                spawn_line.after(layout::spawn_frame),
            )
            .add_systems(OnExit(AppState::InGame), despawn_scoped::<EventLine>)
            .add_systems(
                Update,
                (report_events, post_events, fade_line)
                    .chain()
                    .run_if(in_state(AppState::InGame))
                    // The emitters read run-scoped state (`RunPhase`, units, the boss), so they only
                    // make sense during an expedition. `post_events`/`fade_line` are gated with them
                    // rather than separately: a beat posted as the run ends would otherwise linger on
                    // a screen whose subject no longer exists.
                    .distributive_run_if(in_state(crate::session::RunState::Active)),
            );
    }
}

/// Health fraction at or below which an operative is reported as going down.
///
/// Not zero. `squad::despawn_dead_units` removes the dead on `FixedUpdate`, so a windowed watcher
/// looking for `fraction() == 0` would race the despawn and miss the event that matters most. Firing
/// while the operative is *critical* is also the more useful beat: it is still actionable.
const CRITICAL_FRAC: f32 = 0.25;

/// Emit the beats. **Windowed-only and read-only**, like the rest of `crate::ui`.
///
/// These are the four the game can observe cheaply and reliably today. Each is a genuine tactical
/// beat in Ancker et al.'s sense — a thing the player would act on — rather than a state change that
/// merely happens to be detectable, which is the distinction their "uninformative alert is a false
/// alarm" finding turns on.
#[allow(clippy::too_many_arguments)]
fn report_events(
    mut out: MessageWriter<GameEvent>,
    phase: Option<Res<State<crate::session::RunPhase>>>,
    mut last_phase: Local<Option<crate::session::RunPhase>>,
    critical: Query<
        (Entity, &crate::health::Health, &crate::squad_ai::role::RoleId),
        With<crate::squad::Unit>,
    >,
    capped: Query<Entity, Added<crate::containment::Capped>>,
    boss: Query<(Entity, &crate::enemy::SmileyState)>,
    mut boss_was_angry: Local<bool>,
) {
    // --- Run phase. Subjectless, because a phase is a fact about the run, not about a thing. ---
    if let Some(phase) = phase {
        let now = *phase.get();
        if *last_phase != Some(now) {
            // Skip the very first observation: entering `Locating` at run start is not news, and the
            // objective line already states it.
            if last_phase.is_some() {
                out.write(GameEvent::note(match now {
                    crate::session::RunPhase::Locating => "SEARCHING",
                    crate::session::RunPhase::Containing => "CONTAINMENT IN PROGRESS",
                    crate::session::RunPhase::Extracting => "EXTRACTION OPEN — WALK IT OUT",
                }));
            }
            *last_phase = Some(now);
        }
    }

    // --- An operative in trouble. Deduped by entity, so each one reports once per expedition. ---
    for (entity, health, role) in &critical {
        if health.fraction() <= CRITICAL_FRAC {
            out.write(GameEvent::beat(
                entity,
                format!("{} IS GOING DOWN", super::hud::role_letter(*role)),
            ));
        }
    }

    // --- A nest sealed. `Added<Capped>` fires exactly once, which is the dedupe. ---
    for entity in &capped {
        out.write(GameEvent::beat(entity, "NEST SEALED"));
    }

    // --- The watcher dropping its act. The single most consequential state change in the game. ---
    for (entity, state) in &boss {
        let angry = state.is_angry();
        if angry && !*boss_was_angry {
            out.write(GameEvent::beat(entity, "THE WATCHER IS NO LONGER PRETENDING"));
        }
        *boss_was_angry = angry;
    }
}

fn spawn_line(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    regions: Res<HudRegions>,
) {
    let line = (
        EventLine { since: None },
        text_colored(&theme, &fonts, "", theme.font_body, theme.accent.with_alpha(0.0)),
        Pickable::IGNORE,
    );
    if layout::panel_in(&mut commands, &regions, Region::MidLeft, line).is_none() {
        error!("event line: no layout frame at spawn — tactical beats will not be reported");
    }
}

/// Should this event reach the line?
///
/// Pure, so the whole alert budget is testable without a world. Three gates, and each is one of
/// Ancker et al.'s findings: only [`Severity::Beat`] (delete the uninformative rather than demote
/// it), never a repeat subject (repeated alerts cost acceptance on their own), and never an empty
/// string (an alert carrying nothing is a false alarm by their definition).
fn admits(event: &GameEvent, already_reported: &HashSet<Entity>) -> bool {
    if event.severity != Severity::Beat {
        return false;
    }
    if event.text.trim().is_empty() {
        return false;
    }
    match event.subject {
        Some(e) => !already_reported.contains(&e),
        None => true,
    }
}

fn post_events(
    mut events: MessageReader<GameEvent>,
    mut seen: ResMut<ReportedSubjects>,
    mut sfx: MessageWriter<crate::audio::Sfx>,
    mut lines: Query<(&mut EventLine, &mut Text, &mut TextColor)>,
    theme: Res<UiTheme>,
) {
    // Every message is drained even when it does not qualify, or an unread one would be redelivered
    // next frame and eventually shown out of order.
    let mut posted: Option<GameEvent> = None;
    for event in events.read() {
        if !admits(event, &seen.0) {
            continue;
        }
        if let Some(e) = event.subject {
            seen.0.insert(e);
        }
        // Last qualifying event of the frame wins. With one line there is no honest alternative:
        // queueing would turn this into the scrolling log the alert budget rules out, and showing the
        // first would mean a later, more urgent beat lost to an earlier trivial one.
        posted = Some(event.clone());
    }
    let Some(event) = posted else { return };
    for (mut line, mut text, mut color) in &mut lines {
        line.since = Some(0.0);
        text.0 = format!("{}  {}", event.glyph, event.text);
        color.0 = theme.text;
    }
    // The pip. Non-spatial and uninformative on purpose — that is the condition under which Van der
    // Burg et al. measured the pop-out effect.
    sfx.write(crate::audio::Sfx::MoveOrder);
}

/// Alpha for a line `t` seconds old. `0.0` once it has fully faded.
///
/// Pure and tested: hold, then fade. A line that vanished instantly could be missed entirely, and one
/// that never faded would habituate into furniture.
fn line_alpha(t: f32) -> f32 {
    if t <= HOLD_SECS {
        1.0
    } else {
        (1.0 - (t - HOLD_SECS) / FADE_SECS).clamp(0.0, 1.0)
    }
}

fn fade_line(
    time: Res<Time<Real>>,
    theme: Res<UiTheme>,
    mut lines: Query<(&mut EventLine, &mut Text, &mut TextColor)>,
) {
    let dt = time.delta_secs();
    for (mut line, mut text, mut color) in &mut lines {
        let Some(t) = line.since.as_mut() else { continue };
        *t += dt;
        let alpha = line_alpha(*t);
        if alpha <= 0.0 {
            // Clear the string too, not just the alpha: an invisible node still holding text would
            // keep its layout footprint and could reappear on a theme change.
            line.since = None;
            text.0.clear();
            color.0 = theme.text.with_alpha(0.0);
            continue;
        }
        color.0 = theme.text.with_alpha(alpha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(n: u32) -> Entity {
        Entity::from_raw_u32(n).expect("valid id")
    }

    #[test]
    fn one_subject_reports_once_per_encounter() {
        // Ancker et al. 2017: acceptance falls ~10% for every 5-point rise in the share of REPEATED
        // alerts, independent of volume. The same operative going down twice is the shape that
        // produces that share.
        let mut seen = HashSet::new();
        let e = entity(9);
        let ev = GameEvent::beat(e, "OKAFOR IS DOWN");
        assert!(admits(&ev, &seen));
        seen.insert(e);
        assert!(!admits(&ev, &seen), "a repeat subject must not reach the line");
    }

    #[test]
    fn a_passive_event_has_no_surface_at_all() {
        // Not shown quieter — NOT SHOWN. Their finding is that low-informativeness alerts have to be
        // deleted rather than demoted, because volume is the mechanism and intensity is not.
        let ev = GameEvent {
            severity: Severity::Passive,
            subject: Some(entity(3)),
            text: "A CRAB MOVED".into(),
            glyph: glyph::MET,
        };
        assert!(!admits(&ev, &HashSet::new()));
    }

    #[test]
    fn an_empty_alert_is_a_false_alarm_and_is_refused() {
        // Their words: an uninformative alert "is essentially a false alarm". A blank line would also
        // fire the pip, so the player would hear something happen and see nothing.
        let ev = GameEvent::note("   ");
        assert!(!admits(&ev, &HashSet::new()));
    }

    #[test]
    fn a_subjectless_note_may_repeat_because_it_names_a_phase_not_a_thing() {
        // Phase changes have no entity to dedupe on, and each one is a genuinely new fact. Keying
        // them off would silently drop `CONTAINING` after the first anomaly.
        let seen = HashSet::new();
        assert!(admits(&GameEvent::note("CONTAINMENT BREACHED"), &seen));
        assert!(admits(&GameEvent::note("EXTRACTION OPEN"), &seen));
    }

    #[test]
    fn a_line_holds_then_fades_to_nothing() {
        assert_eq!(line_alpha(0.0), 1.0);
        assert_eq!(line_alpha(HOLD_SECS), 1.0, "it holds long enough to be read");
        assert!(line_alpha(HOLD_SECS + FADE_SECS * 0.5) < 1.0, "then it fades");
        assert_eq!(line_alpha(HOLD_SECS + FADE_SECS), 0.0);
        assert_eq!(line_alpha(999.0), 0.0, "it never comes back");
    }

    #[test]
    fn the_fade_is_monotonic() {
        // A non-monotonic alpha would read as a flicker, which `docs/ui.md` §1.3 reserves for
        // burst-only critical alerts — and `reduce_flashing` exists precisely because sustained
        // flicker is an accessibility cost.
        let mut prev = 1.0;
        for i in 0..200 {
            let a = line_alpha(i as f32 * 0.03);
            assert!(a <= prev + f32::EPSILON, "alpha rose at t={}", i as f32 * 0.03);
            prev = a;
        }
    }

    #[test]
    fn the_hold_outlasts_a_glance_and_the_fade_is_not_a_blink() {
        // Numbers, not vibes: a line the player has to already be looking at is not a notification.
        assert!(HOLD_SECS >= 2.0, "a beat must survive a glance elsewhere");
        assert!(FADE_SECS >= 0.5, "a fade shorter than this reads as a blink");
    }
}
