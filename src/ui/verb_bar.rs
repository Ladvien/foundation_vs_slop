//! **The verb bar** (FVS-B-3) — what the player can *do*, and what it costs.
//!
//! Push 2 shipped all three containment archetypes with no way to invoke any of them. This is the
//! readout for the input layer that fixes that (`crate::selection`): arm a verb — with its key or by
//! clicking its chip — then left-click the target.
//!
//! **Why a bar of named verbs rather than a passive stat block.** Vansteenkiste & Ryan 2013 (*On
//! psychological growth and vulnerability*, DOI 10.1037/a0032359) report that need-supportive
//! environments are the ones "that provide **meaningful choice** or deliver effectance-relevant
//! feedback … Conversely, **controlling reward contingencies**" undermine intrinsic motivation. A row
//! of distinct verbs with visible, spendable charges is choice plus effectance feedback; a hidden
//! multiplier is the other thing. It is the same argument FVS-F-2 makes for unlocks granting verbs
//! rather than numbers — which is why this bar is the surface those unlocks will later extend.
//!
//! # Why the chips are entities now
//!
//! The bar used to be one `Text` node holding `" C  DEVICE x3   [Z] QUARANTINE x1 <"`, with the armed
//! verb marked by ASCII brackets. Two costs. First, a string has no hit target: every verb was
//! keyboard-only, so a mouse-driven player faced a row of things that look like buttons and are not —
//! and Cook's *skill atom* (action → simulation → feedback → modeling) never closes if the obvious
//! action does nothing. Second, `[` and `<` are a weak mark to find while the player is looking at the
//! **world**; a chip carries a border and a luminance step, which are found preattentively.
//!
//! Clicking a chip does **not** write [`ArmedTool`]. It sends [`ArmRequest`], which
//! `selection::arm_tool_input` — the single writer — applies with exactly the same toggle rule as the
//! key, including the `DebugCaptureActive` stand-down. Click and key cannot drift apart.
//!
//! Windowed-only, like the rest of `crate::ui`: `Update` and `OnEnter`/`OnExit` only. It writes no sim
//! state; the one thing it emits is an input intent, which is what a key press is too.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui_widgets::{Activate, Button};

use super::layout::{self, HudRegions, Region};
use super::state::{despawn_scoped, AppState};
use super::theme::{FontAssets, UiTheme};
use super::widgets::{border_all, text_colored};
use crate::containment::{ArmedTool, DeviceSupply, QuarantineSupply};
use crate::laser::WeaponsTight;
use crate::selection::ArmRequest;
use crate::session::{RunPhase, WinCondition};

/// Root marker for the bar (despawned on leaving the game).
#[derive(Component)]
pub struct VerbBarRoot;

/// The objective line above the bar.
#[derive(Component)]
pub struct ObjectiveReadout;

/// The hover hint line, between the objective and the chips. One node, rewritten in place — the same
/// reason the chip labels are: respawning would drop the hover state out from under the cursor.
#[derive(Component)]
pub struct VerbHint;

/// A verb chip, tagged so the styler can find the armed one.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub struct VerbChip(pub Verb);

/// A chip's label node, so charges can be rewritten without respawning the chip — which would drop
/// the hover state out from under a cursor that is mid-click.
#[derive(Component)]
pub struct VerbChipLabel(pub Verb);

/// The verbs the bar offers.
///
/// A closed enum rather than free strings, so a new verb cannot reach the player without someone
/// deciding its key, its wording, and the intent its chip sends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verb {
    Device,
    /// Throw a noisemaker to pull the swarm off a route (`crate::lure`, FVS-B-10).
    Lure,
    Quarantine,
    Cap,
    HoldFire,
    /// The Engineer's sensor drone (`crate::sensor`) — the only thing that turns the minimap on.
    /// Like [`Verb::HoldFire`] it is **not** an `ArmedTool`: there is nothing to aim, so pressing it
    /// acts immediately rather than entering a modal state.
    Sensor,
    /// Advance to contact instead of holding (`squad::PushOrder`). A latched **stance** like
    /// [`Verb::HoldFire`], and the only genuine order in the game — see `squad::PushOrder` for why
    /// the twelve-mode order wheel this replaced was the wrong feature.
    Push,
}

impl Verb {
    pub const ALL: [Verb; 7] =
        [Verb::Device, Verb::Quarantine, Verb::Cap, Verb::Lure, Verb::HoldFire, Verb::Sensor, Verb::Push];

    /// The registry action this verb is bound to. The chip's printed key is read from the **live**
    /// `KeyBindings` through this — see [`Verb::key`].
    pub fn action(self) -> crate::input::Action {
        use crate::input::Action;
        match self {
            Verb::Device => Action::ArmDevice,
            Verb::Quarantine => Action::ArmQuarantine,
            Verb::Cap => Action::ArmCap,
            Verb::Lure => Action::ArmLure,
            Verb::HoldFire => Action::ToggleHoldFire,
            Verb::Sensor => Action::DeploySensor,
            Verb::Push => Action::TogglePush,
        }
    }

    /// The key to print on the chip, from the live binding table.
    ///
    /// This used to be a hardcoded `match` returning `'C'`/`'Z'`/`'X'`/… — a second copy of the
    /// binding, which meant a player who rebound ARM DEVICE to `T` still saw `C  DEVICE x3`. The chip
    /// would have been telling them a key that does nothing, and
    /// `every_verb_states_its_key` exists precisely to guarantee the opposite.
    pub fn key(self, bindings: &crate::input::KeyBindings) -> char {
        bindings.key_char(self.action())
    }

    /// One line saying what the verb *does* — shown while its chip is hovered.
    ///
    /// The chip labels are necessarily terse (`CAP NEST`, `QUARANTINE`), and nothing in the game
    /// explained them. That matters against the one hard number in the controls literature:
    /// Iacovides et al. 2015 had to drop **7 of 31** screened participants — all self-reported FPS
    /// players — for "obviously struggling with the controls" inside 20 minutes.
    ///
    /// **Tooltips are for controls, not for readouts.** `ui::rows` reserves `Row::label()` for this
    /// and nothing implements it, deliberately: `docs/ui.md` §1.4's rule is that a row states its
    /// instruction *inline* (`RAISE OBSERVATION  >= 0.50  now 0.10`), and Llanos & Jørgensen 2011 is
    /// explicit that information which is critical or continuously gauged is the wrong thing to
    /// minimise. Hiding a containment clause behind a hover would undo both. A *verb*, by contrast,
    /// has a meaning the player learns once and then never needs again — which is exactly what a
    /// hover is for.
    pub fn hint(self) -> &'static str {
        match self {
            Verb::Device => "THROW A CAPTURE DEVICE AT AN ANOMALY. TAKES IT ALIVE.",
            Verb::Quarantine => "BOUND A REGION. FOR OUTBREAKS THAT HAVE NO BODY TO CATCH.",
            Verb::Cap => "SEAL A NEST. STOPS REINFORCEMENTS; YIELDS NOTHING.",
            Verb::Lure => "THROW A NOISEMAKER. PULLS THE SWARM; THEY LEARN THE TRICK.",
            Verb::HoldFire => "STOP SHOOTING. SOME ANOMALIES ARE ONLY CONTAINED THIS WAY.",
            Verb::Sensor => "AN ENGINEER DEPLOYS A DRONE. SHOWS THE MAP WHILE IT LASTS.",
            Verb::Push => "THE SELECTION CLOSES ON WHAT IT SEES. THEY HOLD OTHERWISE.",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Verb::Device => "DEVICE",
            Verb::Quarantine => "QUARANTINE",
            Verb::Cap => "CAP NEST",
            Verb::Lure => "THROW LURE",
            Verb::HoldFire => "HOLD FIRE",
            Verb::Sensor => "SENSOR",
            Verb::Push => "ADVANCE",
        }
    }

    /// The intent a click on this chip sends.
    fn request(self) -> ArmRequest {
        match self {
            Verb::Device => ArmRequest::Toggle(ArmedTool::Device),
            Verb::Quarantine => ArmRequest::Toggle(ArmedTool::Quarantine),
            Verb::Cap => ArmRequest::Toggle(ArmedTool::Cap),
            Verb::Lure => ArmRequest::Toggle(ArmedTool::Lure),
            Verb::HoldFire => ArmRequest::ToggleWeaponsTight,
            Verb::Sensor => ArmRequest::DeploySensor,
            Verb::Push => ArmRequest::TogglePush,
        }
    }

    fn armed_by(self, armed: ArmedTool) -> bool {
        matches!(
            (self, armed),
            (Verb::Device, ArmedTool::Device)
                | (Verb::Quarantine, ArmedTool::Quarantine)
                | (Verb::Cap, ArmedTool::Cap)
                | (Verb::Lure, ArmedTool::Lure)
        )
    }
}

pub struct VerbBarPlugin;

impl Plugin for VerbBarPlugin {
    fn build(&self, app: &mut App) {
        // `update_chips` reads the sensor cooldown to print it on the chip, but `sensor::SensorPlugin`
        // is windowed-only — and the UI-liveness test builds the harness app plus `UiPlugin` alone.
        // A missing `Res<T>` is a PANIC in Bevy 0.19, not a skip (`docs/ui.md` §5, trap 2), so the
        // plugin that registers the reader claims the resource. `init_resource` is idempotent, so the
        // real one still wins wherever `SensorPlugin` is present.
        app.init_resource::<crate::sensor::SensorCooldown>();
        app.add_systems(
            OnEnter(AppState::InGame),
            spawn_bar.after(layout::spawn_frame),
        )
        .add_systems(OnExit(AppState::InGame), despawn_scoped::<VerbBarRoot>)
        .add_systems(
            Update,
            (update_chips, style_chips, update_hint, update_objective)
                .run_if(in_state(AppState::InGame)),
        );
    }
}

fn spawn_bar(
    mut commands: Commands,
    theme: Res<UiTheme>,
    fonts: Res<FontAssets>,
    regions: Res<HudRegions>,
) {
    // Bottom-centre, via the region grid.
    //
    // The old bar set `left: Val::Percent(50.0)` with `align_items: Center` and no width, which
    // centres the children *inside* an auto-width node whose LEFT EDGE sits at screen centre — so the
    // whole bar rendered offset right by half its own width. The region owns the centring now, so
    // there is no per-panel arithmetic left to get wrong.
    let root = (
        VerbBarRoot,
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(theme.space_sm),
            ..default()
        },
        Pickable::IGNORE,
    );

    let Some(mut ec) = layout::panel_in(&mut commands, &regions, Region::BottomCenter, root) else {
        error!("verb bar: no layout frame at spawn — the player has no verb readout");
        return;
    };

    ec.with_children(|p| {
        p.spawn((
            ObjectiveReadout,
            text_colored(&theme, &fonts, "", theme.font_body, theme.accent),
            Pickable::IGNORE,
        ));

        // The hover hint. Muted and slightly smaller, because it is teaching rather than reporting —
        // it must not compete with the objective line above it (`docs/ui.md` §2: what a lower density
        // sheds first is what competes for attention).
        p.spawn((
            VerbHint,
            text_colored(&theme, &fonts, "", theme.font_body * 0.85, theme.text_muted),
            Pickable::IGNORE,
        ));

        p.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(theme.space_sm),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|bar| {
            for verb in Verb::ALL {
                bar.spawn((
                    VerbChip(verb),
                    // `bevy_ui_widgets::Button` (already plugged in by `DefaultPlugins`) emits
                    // `Activate` on release. The chip stays pickable — no `Pickable::IGNORE` — so
                    // clicks land on it rather than passing through to a move order.
                    Button,
                    Hovered::default(),
                    Node {
                        padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_xs)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(theme.radius)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                    border_all(theme.panel_border),
                ))
                .observe(move |_: On<Activate>, mut arm: MessageWriter<ArmRequest>| {
                    arm.write(verb.request());
                })
                .with_children(|chip| {
                    chip.spawn((
                        VerbChipLabel(verb),
                        text_colored(&theme, &fonts, "", theme.font_body, theme.text),
                        Pickable::IGNORE,
                    ));
                });
            }
        });
    });
}

/// The text on one chip.
///
/// Pure, so the wording stays unit-testable without a UI tree. An exhausted verb still shows itself
/// at `x0` rather than disappearing — a player who has run out must learn *that*, not wonder where
/// the button went.
fn chip_label(verb: Verb, charges: Option<u32>, tight: bool, cooldown: f32, key: char) -> String {
    match verb {
        // Both stances are latched, not spendable, so they read on/off and never a count. `Push`
        // shares HoldFire's arm because they are the same kind of thing — the `tight` flag carries
        // whichever stance this chip is about (see `update_chips`).
        Verb::HoldFire | Verb::Push => {
            let mark = if tight { "  \u{2022}" } else { "" };
            format!("{key}  {}{mark}", verb.name())
        }
        // The sensor states its COOLDOWN, because that is its price. `docs/ui.md` §1.4: an unmet
        // condition is an instruction — so a cooling chip says how long, never just dims.
        Verb::Sensor => {
            if cooldown > 0.0 {
                format!("{key}  {}  {}s", verb.name(), cooldown.ceil() as u32)
            } else {
                format!("{key}  {}", verb.name())
            }
        }
        _ => match charges {
            Some(n) => format!("{key}  {} x{n}", verb.name()),
            None => format!("{key}  {}", verb.name()),
        },
    }
}

fn charges_for(verb: Verb, devices: u32, quarantines: u32, lures: u32) -> Option<u32> {
    match verb {
        Verb::Device => Some(devices),
        Verb::Quarantine => Some(quarantines),
        // The lure IS a per-expedition supply, so it shows a charge count like the other two. Its
        // habituation is deliberately NOT shown here — a second number on the chip would imply two
        // resources when there is one, and the swarm's boredom is meant to be felt, not read.
        Verb::Lure => Some(lures),
        // Cap has no supply; HoldFire is a stance; the Sensor's cost is a COOLDOWN, not a charge
        // (see `crate::sensor` on why it is time rather than an economy).
        Verb::Cap | Verb::HoldFire | Verb::Sensor | Verb::Push => None,
    }
}

fn update_chips(
    devices: Res<DeviceSupply>,
    quarantines: Res<QuarantineSupply>,
    lures: Res<crate::lure::LureSupply>,
    tight: Res<WeaponsTight>,
    sensor_cd: Res<crate::sensor::SensorCooldown>,
    pushers: Query<(), (With<crate::squad::Unit>, With<crate::squad::PushOrder>)>,
    bindings: Res<crate::input::KeyBindings>,
    mut labels: Query<(&VerbChipLabel, &mut Text)>,
) {
    // Any operative advancing lights the chip. It is a squad-wide readout of a per-unit order, which
    // is honest at this altitude: the roster chips are where per-operative state lives.
    let pushing = pushers.iter().next().is_some();
    for (label, mut text) in &mut labels {
        // Each stance reads its OWN latch. Passing `tight` for both would make ADVANCE mirror
        // HOLD FIRE — two chips showing one state, which is worse than showing none.
        let latched = match label.0 {
            Verb::HoldFire => tight.0,
            Verb::Push => pushing,
            _ => false,
        };
        let want = chip_label(
            label.0,
            charges_for(label.0, devices.0, quarantines.0, lures.0),
            latched,
            sensor_cd.0,
            label.0.key(&bindings),
        );
        if text.0 != want {
            text.0 = want;
        }
    }
}

/// Armed / hovered / exhausted styling.
///
/// The armed chip is marked by **luminance and border**, never by a hue change, so it is findable in
/// peripheral vision while the player looks at the world — the same encoding rule the rest of the HUD
/// follows (`docs/ui.md` §1.3). An exhausted verb dims rather than vanishing.
///
/// Runs every frame because `ArmedTool` changes are not per-entity change-detectable here and there
/// are four chips; the writes are still guarded so an unchanged chip does not churn its `Node`.
fn style_chips(
    theme: Res<UiTheme>,
    armed: Res<ArmedTool>,
    tight: Res<WeaponsTight>,
    devices: Res<DeviceSupply>,
    quarantines: Res<QuarantineSupply>,
    lures: Res<crate::lure::LureSupply>,
    pushers: Query<(), (With<crate::squad::Unit>, With<crate::squad::PushOrder>)>,
    mut chips: Query<(&VerbChip, &Hovered, &mut BackgroundColor, &mut BorderColor)>,
    mut labels: Query<(&VerbChipLabel, &mut TextColor)>,
) {
    let pushing = pushers.iter().next().is_some();
    let lit = |verb: Verb| {
        verb.armed_by(*armed)
            || (verb == Verb::HoldFire && tight.0)
            || (verb == Verb::Push && pushing)
    };
    let spent = |verb: Verb| charges_for(verb, devices.0, quarantines.0, lures.0) == Some(0);

    for (chip, hovered, mut bg, mut border) in &mut chips {
        let verb = chip.0;
        let want_bg = if lit(verb) {
            theme.panel_border.with_alpha(0.30)
        } else if hovered.0 {
            theme.panel_border.with_alpha(0.16)
        } else {
            theme.panel
        };
        let want_border = if lit(verb) {
            theme.accent
        } else if spent(verb) {
            theme.panel_border.with_alpha(0.25)
        } else {
            theme.panel_border
        };

        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want = border_all(want_border);
        if border.top != want.top {
            *border = want;
        }
    }

    for (label, mut color) in &mut labels {
        let verb = label.0;
        let want = if lit(verb) {
            theme.accent
        } else if spent(verb) {
            theme.text_muted
        } else {
            theme.text
        };
        if color.0 != want {
            color.0 = want;
        }
    }
}

/// What the player is supposed to be doing, derived from the phase.
///
/// Reads `RunPhase`, which is exactly what that state is for — presentation. It must never gate pinned
/// gameplay (see `session::advance_run_phase`), and an `Update`-side readout is the shape that cannot.
///
/// **This line must always name a next goal.** Phan, Keebler & Chaparro 2016's validation of the GUESS
/// (DOI 10.1177/0018720816669646, N=629) found *"I always know my next goal when I finish an event"*
/// the lowest-scoring item in the entire usability subscale (M=5.46 of 7) — the industry's weakest
/// link. So the instant the quota is met this switches to the extraction instruction instead of
/// continuing to report a count.
fn objective_line(
    win: WinCondition,
    phase: RunPhase,
    contained: u32,
    nests: (usize, usize),
) -> String {
    // Nest progress rides alongside the objective rather than in its own panel: capping is a verb with
    // no other feedback at all (`Capped` grants nothing and is deliberately invisible — FVS-B-7), so
    // without this the player seals a nest and sees literally nothing happen.
    let (capped, total) = nests;
    let sites = if total > 0 { format!("   NESTS {capped}/{total}") } else { String::new() };
    match win {
        WinCondition::SurviveTicks(_) => format!("HOLD THE SITE{sites}"),
        WinCondition::ExtractContained { count } => match phase {
            RunPhase::Locating => format!("LOCATE AND CONTAIN {count} ANOMALY(S){sites}"),
            RunPhase::Containing => format!("CONTAINING — {contained}/{count} SECURED{sites}"),
            RunPhase::Extracting => format!("RETURN TO THE EXTRACTION POINT{sites}"),
        },
    }
}

/// Which hint to show, given what is hovered and what is armed.
///
/// Pure, so the precedence is testable. **Hover wins over armed**, because a player moving the cursor
/// along the bar is asking "what is this one?" — answering with the verb they already armed would make
/// the line unusable for the thing it exists for. With neither, it is empty: `docs/ui.md` §1.2 —
/// a widget that supports no decision is noise, and this one has nothing to say until it does.
fn hint_text(hovered: Option<Verb>, armed: Option<Verb>) -> &'static str {
    match hovered.or(armed) {
        Some(v) => v.hint(),
        None => "",
    }
}

fn update_hint(
    armed: Res<ArmedTool>,
    chips: Query<(&VerbChip, &Hovered)>,
    mut hints: Query<&mut Text, With<VerbHint>>,
) {
    // SORT-OK: at most one chip can be hovered at a time (they do not overlap), so `find` has a
    // unique answer and there is no order to decide.
    let hovered = chips.iter().find(|(_, h)| h.0).map(|(c, _)| c.0);
    let armed_verb = Verb::ALL.iter().copied().find(|v| v.armed_by(*armed));
    let want = hint_text(hovered, armed_verb);
    for mut text in &mut hints {
        if text.0 != want {
            text.0 = want.to_string();
        }
    }
}

fn update_objective(
    win: Res<WinCondition>,
    phase: Res<State<RunPhase>>,
    secured: Res<crate::containment::SiteSecured>,
    contained: Query<(), With<crate::containment::Contained>>,
    mut readout: Query<&mut Text, With<ObjectiveReadout>>,
) {
    let line = objective_line(
        *win,
        *phase.get(),
        contained.iter().count() as u32,
        (secured.capped, secured.total),
    );
    for mut text in &mut readout {
        if text.0 != line {
            text.0 = line.clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exhausted_verb_still_shows_itself_at_zero() {
        // Hiding it would teach the player the verb does not exist rather than that it is spent.
        assert!(chip_label(Verb::Device, Some(0), false, 0.0, 'C').contains("DEVICE x0"));
        assert!(chip_label(Verb::Quarantine, Some(0), false, 0.0, 'Z').contains("QUARANTINE x0"));
    }

    #[test]
    fn hold_fire_reads_as_a_stance_not_a_charge() {
        let off = chip_label(Verb::HoldFire, None, false, 0.0, 'F');
        let on = chip_label(Verb::HoldFire, None, true, 0.0, 'F');
        assert!(off.contains("HOLD FIRE"));
        assert!(on.contains('\u{2022}'), "the latched stance is marked: {on}");
        assert_ne!(off, on);
        // Never a count — it is latched, not spent.
        assert!(!on.contains("HOLD FIRE x"));
    }

    #[test]
    fn every_verb_states_its_key() {
        // The chip is clickable AND keyed. A chip that did not name its key would teach the mouse
        // player that the keyboard route does not exist.
        let b = crate::input::KeyBindings::default();
        for v in Verb::ALL {
            let l = chip_label(v, charges_for(v, 3, 1, 2), false, 0.0, v.key(&b));
            assert!(l.starts_with(v.key(&b)), "{v:?} chip must lead with its key: {l}");
        }

        // ...and the key it leads with is the LIVE one, not a hardcoded copy. Rebind and the chip
        // must follow, or the bar is telling the player a key that does nothing.
        let mut rebound = crate::input::KeyBindings::default();
        rebound
            .rebind(
                Verb::Device.action(),
                crate::input::Binding::one(crate::input::Chord::plain(bevy::prelude::KeyCode::KeyT)),
            )
            .expect("T is free");
        assert_eq!(Verb::Device.key(&rebound), 'T');
        let l = chip_label(Verb::Device, Some(3), false, 0.0, Verb::Device.key(&rebound));
        assert!(l.starts_with('T'), "the chip must follow the rebind: {l}");
    }

    #[test]
    fn a_cooling_sensor_says_how_long_rather_than_just_dimming() {
        // `docs/ui.md` §1.4's strongest rule: an unmet condition is an INSTRUCTION. A chip that only
        // greyed out would leave the player unable to tell "on cooldown" from "no Engineer selected"
        // from "this verb is broken" — three different states with three different responses.
        let ready = chip_label(Verb::Sensor, None, false, 0.0, 'V');
        assert_eq!(ready, "V  SENSOR", "a ready sensor states no time");

        let cooling = chip_label(Verb::Sensor, None, false, 7.2, 'V');
        assert!(cooling.contains("SENSOR"), "still names the verb: {cooling}");
        assert!(cooling.contains('8'), "rounds UP so it never promises early: {cooling}");
        assert_ne!(ready, cooling);
    }

    #[test]
    fn the_sensor_never_reads_as_a_charge() {
        // Its cost is time, not a pool. An `x1` would advertise an economy that does not exist and
        // send the player to the requisition screen looking for it.
        for cd in [0.0_f32, 1.0, 29.0] {
            let l = chip_label(Verb::Sensor, None, false, cd, 'V');
            assert!(!l.contains(" x"), "{l} reads as a spendable charge");
        }
        assert_eq!(charges_for(Verb::Sensor, 9, 9, 2), None);
    }

    #[test]
    fn the_sensor_is_a_request_not_an_armed_tool() {
        // Like HOLD FIRE and unlike the three containment verbs: there is nothing to aim, so pressing
        // it must act immediately rather than entering a modal state the player has to escape.
        assert_eq!(Verb::Sensor.request(), ArmRequest::DeploySensor);
        for tool in [ArmedTool::None, ArmedTool::Device, ArmedTool::Quarantine, ArmedTool::Cap] {
            assert!(!Verb::Sensor.armed_by(tool), "the sensor must never read as armed");
        }
    }

    #[test]
    fn hovering_a_chip_explains_it_and_hover_beats_armed() {
        // The line exists so a player can ask "what is this one?" while moving along the bar. If the
        // armed verb won, the answer would be the verb they already understand well enough to have
        // chosen — which makes the line useless for its only job.
        assert_eq!(hint_text(Some(Verb::Cap), None), Verb::Cap.hint());
        assert_eq!(
            hint_text(Some(Verb::Cap), Some(Verb::Device)),
            Verb::Cap.hint(),
            "hover must win over armed"
        );
        assert_eq!(hint_text(None, Some(Verb::Device)), Verb::Device.hint());
    }

    #[test]
    fn the_hint_is_empty_when_there_is_nothing_to_explain() {
        // `docs/ui.md` §1.2 — a widget supporting no decision is noise, and noise is not neutral. A
        // permanent placeholder line under the objective would cost a row of attention for nothing.
        assert_eq!(hint_text(None, None), "");
    }

    #[test]
    fn every_verb_explains_itself_distinctly() {
        // A shared or missing hint would teach the player that two verbs are the same thing — worse
        // than no hint at all, because it is confidently wrong.
        for (i, a) in Verb::ALL.iter().enumerate() {
            let h = a.hint();
            assert!(!h.trim().is_empty(), "{a:?} has no hint");
            assert!(h.len() > 20, "{a:?}'s hint says too little to be worth the row: {h}");
            for b in &Verb::ALL[i + 1..] {
                assert_ne!(h, b.hint(), "{a:?} and {b:?} share a hint");
            }
        }
    }

    #[test]
    fn the_hint_says_what_the_verb_does_not_what_it_is_called() {
        // A hint that only restated the chip label would be pure noise. Each one has to name an
        // EFFECT — which is also FVS-L-1's copy rule (say the instruction, not the status) applied to
        // teaching copy.
        for v in Verb::ALL {
            let h = v.hint();
            assert_ne!(h, v.name(), "{v:?}'s hint just repeats its label");
            assert!(
                h.split_whitespace().count() >= 5,
                "{v:?}'s hint is not a sentence: {h}"
            );
        }
    }

    #[test]
    fn advance_reads_as_a_stance_not_a_charge() {
        // Same contract HOLD FIRE has, and it must not drift from it: both are latched orders the
        // player leaves on, not consumables. An `x1` would advertise an economy that does not exist.
        let off = chip_label(Verb::Push, None, false, 0.0, 'G');
        let on = chip_label(Verb::Push, None, true, 0.0, 'G');
        assert!(off.contains("ADVANCE"));
        assert!(on.contains('\u{2022}'), "the latched stance is marked: {on}");
        assert_ne!(off, on);
        assert!(!on.contains("ADVANCE x"), "never a count");
        assert_eq!(charges_for(Verb::Push, 9, 9, 2), None);
    }

    #[test]
    fn the_two_stances_are_told_apart_by_their_own_latches() {
        // `update_chips` used to pass `tight` for every stance. With two of them that would make
        // ADVANCE mirror HOLD FIRE — two chips reporting one state, which is worse than reporting
        // none, because it looks like information.
        assert_ne!(
            chip_label(Verb::HoldFire, None, true, 0.0, 'F'),
            chip_label(Verb::Push, None, true, 0.0, 'G'),
            "the two stances must not render identically when both are on"
        );
    }

    #[test]
    fn advance_is_a_request_not_an_armed_tool() {
        assert_eq!(Verb::Push.request(), ArmRequest::TogglePush);
        for tool in [ArmedTool::None, ArmedTool::Device, ArmedTool::Quarantine, ArmedTool::Cap] {
            assert!(!Verb::Push.armed_by(tool), "a stance is never an armed tool");
        }
    }

    #[test]
    fn verb_keys_are_unique() {
        let bindings = crate::input::KeyBindings::default();
        // Two verbs on one key would make one of them permanently unreachable from the keyboard.
        for (i, a) in Verb::ALL.iter().enumerate() {
            for b in &Verb::ALL[i + 1..] {
                assert_ne!(
                    a.key(&bindings),
                    b.key(&bindings),
                    "{a:?} and {b:?} share key {}",
                    a.key(&bindings)
                );
            }
        }
    }

    #[test]
    fn a_click_sends_the_same_intent_the_key_does() {
        // Pins the mapping `selection::arm_tool_input` applies, so a new verb cannot get a chip
        // without getting the matching arm request — which is how the click and the key stay one
        // behaviour with one writer instead of two that drift.
        assert_eq!(Verb::Device.request(), ArmRequest::Toggle(ArmedTool::Device));
        assert_eq!(Verb::Quarantine.request(), ArmRequest::Toggle(ArmedTool::Quarantine));
        assert_eq!(Verb::Cap.request(), ArmRequest::Toggle(ArmedTool::Cap));
        assert_eq!(Verb::HoldFire.request(), ArmRequest::ToggleWeaponsTight);
    }

    #[test]
    fn only_the_armed_verb_reads_as_armed() {
        assert!(Verb::Device.armed_by(ArmedTool::Device));
        assert!(!Verb::Quarantine.armed_by(ArmedTool::Device));
        for v in Verb::ALL {
            assert!(!v.armed_by(ArmedTool::None), "{v:?} must not read as armed when nothing is");
        }
        // Hold fire is a stance, not an `ArmedTool`, so it is never "armed" by this path.
        assert!(!Verb::HoldFire.armed_by(ArmedTool::Cap));
    }

    #[test]
    fn the_objective_names_the_extraction_only_once_the_quota_is_met() {
        let win = WinCondition::ExtractContained { count: 1 };
        assert!(objective_line(win, RunPhase::Locating, 0, (0, 0)).contains("LOCATE"));
        assert!(objective_line(win, RunPhase::Containing, 0, (0, 0)).contains("0/1"));
        assert!(objective_line(win, RunPhase::Extracting, 1, (0, 0)).contains("EXTRACTION"));
    }

    #[test]
    fn the_objective_is_never_blank_in_any_phase() {
        // GUESS's weakest item is "I always know my next goal when I finish an event". A blank
        // objective line is that failure in its purest form.
        for win in [
            WinCondition::SurviveTicks(100),
            WinCondition::ExtractContained { count: 2 },
        ] {
            for phase in [RunPhase::Locating, RunPhase::Containing, RunPhase::Extracting] {
                let l = objective_line(win, phase, 0, (1, 3));
                assert!(!l.trim().is_empty(), "{win:?}/{phase:?} left the player with no goal");
            }
        }
    }

    #[test]
    fn nest_progress_shows_only_when_there_are_nests() {
        // A "NESTS 0/0" on a level with none would be noise, and capping is otherwise INVISIBLE
        // feedback (`Capped` grants nothing by design), so this line is the verb's only acknowledgement.
        let win = WinCondition::ExtractContained { count: 1 };
        assert!(!objective_line(win, RunPhase::Locating, 0, (0, 0)).contains("NESTS"));
        assert!(objective_line(win, RunPhase::Locating, 0, (2, 4)).contains("NESTS 2/4"));
    }

    #[test]
    fn the_placeholder_win_never_tells_the_player_to_extract() {
        // `SurviveTicks` has no quota, so an "extract" instruction would be a lie. Pins the pairing
        // between the win variant and the copy.
        let win = WinCondition::SurviveTicks(100);
        for phase in [RunPhase::Locating, RunPhase::Containing, RunPhase::Extracting] {
            assert_eq!(objective_line(win, phase, 0, (0, 0)), "HOLD THE SITE");
        }
    }
}
