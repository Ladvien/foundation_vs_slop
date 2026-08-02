//! **Activities — Dr. Lindqvist, Paratherapist**, and the two things she can do for a squad.
//!
//! # The room this closes
//!
//! `layout::AreaId` names the process risk in its own doc comment: the second five areas were
//! authored space-first, and *"the repo's named top process risk is shipping a room with no verb in
//! it."* Activities was one of them, and it was also the one with a person already standing in it
//! whose canon job title names a shipped mechanic. `assets/site/staff.ron:57` posts Dr. Lindqvist
//! here, `Paratherapist` being the Foundation's word for someone who treats what the field does to
//! people, and `docs/lore/2026-08-02-site-67-recommissioned.md` calls her one of only two staff who
//! *"stand where the game already has a system"*.
//!
//! # The mechanic she treats did not exist
//!
//! Both that roster comment and the lore doc claimed operatives carry FEAR between expeditions. They
//! did not: `Drives` is run-scoped, absent from `SaveGame`, and site avatars carry none at all, so
//! fear reset to zero every expedition and there was nothing for a therapist to work on.
//!
//! What *does* persist is `SquadKnowledge`, so [`crate::knowledge::Knowledge::strain`] is where the
//! cost of a career had to live — see that field for why it belongs inside `Knowledge` rather than
//! beside it. Strain is the design doc §6.2 counter-pressure to veteran lock-in, made concrete:
//!
//! > *"if one operative accumulates everything, the player will always pick them and the others rot.
//! > A counter-pressure is needed — fatigue, assignment limits, or simply that fear accumulates
//! > alongside knowledge and a veteran is the most afraid."*
//!
//! # Two verbs, and the second one is the interesting one
//!
//! **A session** spends time and gives strain back. Routine, cheap, and the reason to walk here.
//!
//! **A deep debrief** talks an operative down from the `Lethal` belief they hold most confidently —
//! and takes that confidence with it. `Knowledge::fear_scale` reads exactly the number it burns, so
//! the relief is real and so is the price: below the weakest provenance the belief is *removed*, not
//! left faint, and `can_read_rule` goes dark with it. That is design doc §3.4 as a button —
//! *"understanding a thing is what makes it frightening, and also the only way to contain it."*
//!
//! Windowed-only, and it writes only `SquadKnowledge` — the same persisted table
//! `antagonist::purge_disproven` edits from the records office, which is the precedent for a hub verb
//! that changes what the squad knows.

use bevy::prelude::*;

use crate::knowledge::{roster::SquadKnowledge, Knowledge};
use crate::ui::state::{despawn_scoped, AppState};

/// Root marker for the paratherapy readout.
#[derive(Component)]
pub struct ActivitiesPanel;

/// The node whose children are the readout's rows.
#[derive(Component)]
pub struct ActivitiesReadout;

/// Which of the two verbs a button offers.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub enum TherapyVerb {
    /// Give strain back.
    Session,
    /// Burn a `Lethal` belief's confidence to buy more relief than a session can.
    DeepDebrief,
}

impl TherapyVerb {
    pub const ALL: [TherapyVerb; 2] = [TherapyVerb::Session, TherapyVerb::DeepDebrief];

    pub fn label(self) -> &'static str {
        match self {
            TherapyVerb::Session => "SESSION",
            TherapyVerb::DeepDebrief => "DEEP DEBRIEF",
        }
    }

    pub fn action(self) -> crate::input::Action {
        match self {
            TherapyVerb::Session => crate::input::Action::TherapySession,
            TherapyVerb::DeepDebrief => crate::input::Action::DeepDebrief,
        }
    }
}

/// A button's label node, so the text can be rewritten without respawning the button — the rule
/// `review::BuyButton` records, because `sync_rows` despawns a panel's row subtree on every change
/// and a button inside it would die under the cursor mid-click.
#[derive(Component, Clone, Copy)]
pub struct TherapyButtonLabel(pub TherapyVerb);

/// A request for one therapy verb, from **either** input route — the key or the panel button.
///
/// A message for the reason `review::PurchaseRequest` is: [`therapy_input`] stays the single writer
/// of strain, so the click and the key cannot drift apart and grow two copies of the rule about who
/// is treated.
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct TherapyRequest(pub TherapyVerb);

/// **Who gets treated: the most strained operative on the roster.**
///
/// A *pick over a table*, so it needs a stable total key — `(strain, slot)`. Strain is an `f32` and
/// two operatives can genuinely tie (both back from the same first expedition, both at 0.17), so the
/// roster slot is what breaks it. `SquadKnowledge::members` is a fixed array indexed by slot, which
/// makes that key total and machine-independent by construction.
///
/// Written as an explicit loop rather than `max_by_key`: `tests/determinism_lint.rs` scans for that
/// method by name and would demand a declaration, and a comparator over a table is precisely the
/// shape it exists to catch.
pub fn most_strained(table: &SquadKnowledge) -> Option<usize> {
    let mut best: Option<(f32, usize)> = None;
    for (slot, k) in table.members.iter().enumerate() {
        if k.strain <= 0.0 {
            continue;
        }
        // Reversed on the slot so a tie goes to the LOWER slot — the same "first on the roster"
        // reading the roster screen shows, rather than the last.
        let key = (k.strain, std::cmp::Reverse(slot));
        if best.is_none_or(|(bs, bi)| key > (bs, std::cmp::Reverse(bi))) {
            best = Some((k.strain, slot));
        }
    }
    best.map(|(_, slot)| slot)
}

/// Apply one therapy verb. **The single writer of strain at the Site.**
pub fn therapy_input(
    actions: crate::input::Actions,
    mut requests: MessageReader<TherapyRequest>,
    mut table: ResMut<SquadKnowledge>,
    sim: Res<crate::sim::SimTuning>,
    roster: Res<crate::squad_ai::persona::PersonaRoster>,
) {
    // Key first, then drained messages — the idiom `requisition_input` sets, and draining is what
    // makes a double-click impossible to turn into two sessions.
    let mut want = TherapyVerb::ALL
        .iter()
        .copied()
        .find(|v| actions.just_pressed(v.action()));
    for req in requests.read() {
        want = Some(req.0);
    }
    let Some(verb) = want else { return };

    let Some(slot) = most_strained(&table) else {
        // Named, not silent: `docs/ui.md` §1.4's rule that an empty state must say so applies to a
        // refused action just as much as to an empty panel.
        info!("activities: nobody on the roster is carrying anything to talk about");
        return;
    };
    let plates = roster.name_plates();
    let who = plates
        .get(slot)
        .cloned()
        .unwrap_or_else(|| format!("OPERATIVE {slot}"));
    let k: &mut Knowledge = &mut table.members[slot];
    match verb {
        TherapyVerb::Session => {
            if k.relieve_strain(sim.strain.relief_per_session) {
                info!("activities: Lindqvist sat with {who} — strain now {:.2}", k.strain);
            }
        }
        TherapyVerb::DeepDebrief => {
            // The order matters and is the whole trade: relief is granted only if a belief was
            // actually burned. A debrief with nothing to talk through must not be free relief.
            match k.deep_debrief(sim.strain.debrief_confidence_loss) {
                Some(subject) => {
                    k.relieve_strain(sim.strain.relief_per_session * 2.0);
                    info!(
                        "activities: {who} was debriefed on {subject:?} — they believe it less, and \
                         are less afraid of it. Strain now {:.2}",
                        k.strain
                    );
                }
                None => info!(
                    "activities: {who} holds no lethal belief to be talked out of — a session is \
                     what is on offer"
                ),
            }
        }
    }
}

/// A roster slot's name plate, or an honest placeholder — never a blank cell, which would read as
/// a rendering fault rather than as an unstaffed slot.
fn name_of(names: &[String], slot: usize) -> String {
    names.get(slot).cloned().unwrap_or_else(|| format!("OPERATIVE {slot}"))
}

/// What the readout says, as rows. Pure, so the wording and the emphasis are testable without an
/// `App` — the property every panel in this codebase has.
pub fn activities_rows(
    table: &SquadKnowledge,
    names: &[String],
    bindings: &crate::input::KeyBindings,
) -> Vec<crate::ui::rows::Row> {
    use crate::ui::rows::Row;
    let mut rows = vec![Row::header("PARATHERAPY — DR. LINDQVIST")];

    let mut any = false;
    for (slot, k) in table.members.iter().enumerate() {
        if k.strain <= 0.0 {
            continue;
        }
        any = true;
        // Strain as a bar rather than a number: it is a feeling, not a quantity the player does
        // arithmetic on, and `ui::rows` renders these the same way the containment clauses do.
        let filled = (k.strain * 5.0).round().clamp(0.0, 5.0) as usize;
        let bar: String = "▓".repeat(filled) + &"░".repeat(5 - filled);
        // `unmet` for the worst-off: an alert emphasis reads as "this is the one to deal with",
        // which is exactly what it is.
        rows.push(if Some(slot) == most_strained(table) {
            Row::unmet(name_of(names, slot), bar)
        } else {
            Row::met(name_of(names, slot), bar)
        });
    }
    if !any {
        // Never an empty panel — `docs/ui.md` §1.4. And the wording is the fiction: a rested squad is
        // a good state, not a missing readout.
        rows.push(Row::note("NOBODY IS CARRYING ANYTHING. THE ROOM IS QUIET."));
        return rows;
    }

    rows.push(Row::note(match most_strained(table) {
        Some(slot) => {
            let k = &table.members[slot];
            if k.knows_anything() {
                "A DEBRIEF TRADES WHAT THEY KNOW FOR WHAT IT COSTS THEM"
            } else {
                "A SESSION IS ALL THERE IS TO OFFER — THEY KNOW NOTHING TO UNLEARN"
            }
        }
        None => "",
    }));
    let _ = bindings;
    rows
}

/// What one button reads. **It states its key**, the rule
/// `verb_bar::every_verb_states_its_key` sets and `review::buy_button_label` follows, read from the
/// live table so a rebound key is never advertised wrong.
pub fn therapy_button_label(
    verb: TherapyVerb,
    table: &SquadKnowledge,
    bindings: &crate::input::KeyBindings,
) -> String {
    let key = bindings.key_char(verb.action());
    let Some(slot) = most_strained(table) else {
        return format!("{key}  {} — NOBODY TO SEE", verb.label());
    };
    match verb {
        TherapyVerb::Session => format!("{key}  {}", verb.label()),
        // An unavailable action states WHY, rather than only dimming — FVS-L-1's rule that an unmet
        // condition reads as an instruction.
        TherapyVerb::DeepDebrief if !table.members[slot].knows_anything() => {
            format!("{key}  {} — NOTHING TO UNLEARN", verb.label())
        }
        TherapyVerb::DeepDebrief => format!("{key}  {} — COSTS A BELIEF", verb.label()),
    }
}

fn spawn_panel(
    mut commands: Commands,
    theme: Res<crate::ui::theme::UiTheme>,
    fonts: Res<crate::ui::theme::FontAssets>,
    regions: Res<crate::ui::layout::HudRegions>,
) {
    let panel = (
        ActivitiesPanel,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(theme.space_xs),
            padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_sm)),
            min_width: Val::Px(300.0),
            ..default()
        },
        BackgroundColor(theme.panel),
        crate::ui::widgets::border_all(theme.panel_border),
        Pickable::IGNORE,
    );
    // `BottomLeft` is free in this room: requisition claims it, and requisition is a different room.
    // That is the invariant `presence::no_two_panels_in_one_room_claim_the_same_hud_region` pins,
    // and it is only safe BECAUSE the panels are room-gated.
    let Some(mut ec) = crate::ui::layout::panel_in(
        &mut commands,
        &regions,
        crate::ui::layout::Region::BottomLeft,
        panel,
    ) else {
        error!("activities: no layout frame at spawn — the paratherapy readout is not shown");
        return;
    };
    ec.with_children(|p| {
        p.spawn((
            ActivitiesReadout,
            crate::ui::rows::RowPanel::default(),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space_xs),
                ..default()
            },
            Pickable::IGNORE,
        ));
        p.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(theme.space_xs),
                margin: UiRect::top(Val::Px(theme.space_sm)),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|col| {
            for verb in TherapyVerb::ALL {
                col.spawn((
                    verb,
                    bevy::ui_widgets::Button,
                    bevy::picking::hover::Hovered::default(),
                    Node {
                        padding: UiRect::axes(Val::Px(theme.space_md), Val::Px(theme.space_xs)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(theme.radius)),
                        ..default()
                    },
                    BackgroundColor(theme.panel),
                    crate::ui::widgets::border_all(theme.panel_border),
                ))
                .observe(
                    move |_: On<bevy::ui_widgets::Activate>,
                          mut out: MessageWriter<TherapyRequest>| {
                        out.write(TherapyRequest(verb));
                    },
                )
                .with_children(|b| {
                    b.spawn((
                        TherapyButtonLabel(verb),
                        crate::ui::widgets::text_colored(
                            &theme,
                            &fonts,
                            "",
                            theme.font_body,
                            theme.text,
                        ),
                        Pickable::IGNORE,
                    ));
                });
            }
        });
    });
}

fn update_panel(
    mut commands: Commands,
    theme: Res<crate::ui::theme::UiTheme>,
    fonts: Res<crate::ui::theme::FontAssets>,
    table: Res<SquadKnowledge>,
    roster: Res<crate::squad_ai::persona::PersonaRoster>,
    bindings: Res<crate::input::KeyBindings>,
    mut panels: Query<(Entity, &mut crate::ui::rows::RowPanel), With<ActivitiesReadout>>,
    mut labels: Query<(&TherapyButtonLabel, &mut Text)>,
) {
    let rows = activities_rows(&table, &roster.name_plates(), &bindings);
    for (entity, mut panel) in &mut panels {
        crate::ui::rows::sync_rows(&mut commands, entity, &mut panel, &theme, &fonts, rows.clone());
    }
    for (label, mut text) in &mut labels {
        let want = therapy_button_label(label.0, &table, &bindings);
        if text.0 != want {
            text.0 = want;
        }
    }
}

/// Availability styling. **Luminance and border, never hue** — `docs/ui.md` §1.3.
fn style_therapy_buttons(
    theme: Res<crate::ui::theme::UiTheme>,
    table: Res<SquadKnowledge>,
    mut buttons: Query<(
        &TherapyVerb,
        &bevy::picking::hover::Hovered,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut labels: Query<(&TherapyButtonLabel, &mut TextColor)>,
) {
    let available = |verb: TherapyVerb| match most_strained(&table) {
        None => false,
        Some(slot) => match verb {
            TherapyVerb::Session => true,
            TherapyVerb::DeepDebrief => table.members[slot].knows_anything(),
        },
    };
    for (verb, hovered, mut bg, mut border) in &mut buttons {
        let on = available(*verb);
        let want_bg = if hovered.0 && on {
            theme.panel_border.with_alpha(0.16)
        } else {
            theme.panel
        };
        let want_border = if on {
            theme.panel_border
        } else {
            theme.panel_border.with_alpha(0.25)
        };
        if bg.0 != want_bg {
            bg.0 = want_bg;
        }
        let want = crate::ui::widgets::border_all(want_border);
        if border.top != want.top {
            *border = want;
        }
    }
    for (label, mut color) in &mut labels {
        let want = if available(label.0) { theme.text } else { theme.text_muted };
        if color.0 != want {
            color.0 = want;
        }
    }
}

/// The activities room, wired. **Windowed-only**, like the rest of the hub's UI.
pub struct ActivitiesPlugin;

impl Plugin for ActivitiesPlugin {
    fn build(&self, app: &mut App) {
        crate::input::claim_bindings(app);
        crate::site::claim_current_area(app);
        app.add_message::<TherapyRequest>()
            // The panel opens itself when the player walks into Activities. Presence OFFERS; the key
            // still ACTS — `therapy_input` below is gated on the Site, not on the room.
            .add_systems(
                Update,
                spawn_panel
                    .after(crate::ui::layout::spawn_frame)
                    .run_if(super::panel_wanted::<ActivitiesPanel>(
                        super::AreaId::Activities,
                    )),
            )
            .add_systems(
                Update,
                despawn_scoped::<ActivitiesPanel>.run_if(super::panel_stale::<ActivitiesPanel>(
                    super::AreaId::Activities,
                )),
            )
            .add_systems(OnExit(AppState::Site), despawn_scoped::<ActivitiesPanel>)
            .add_systems(
                Update,
                (therapy_input, update_panel, style_therapy_buttons)
                    .run_if(in_state(AppState::Site)),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge::{Claim, Provenance, Subject};

    fn table_with(strains: &[f32]) -> SquadKnowledge {
        let mut t = SquadKnowledge::default();
        for (i, s) in strains.iter().enumerate() {
            t.members[i].strain = *s;
        }
        t
    }

    /// The worst-off operative is treated, and a tie goes to the earlier roster slot.
    ///
    /// A tie is the COMMON case, not an edge one: everybody back from the same first expedition
    /// carries exactly `per_expedition`. Left to a comparator over a table this would be decided by
    /// iteration order, which is the failure `tests/determinism_lint.rs` exists for.
    #[test]
    fn the_most_strained_operative_is_seen_and_ties_go_to_the_first_slot() {
        assert_eq!(most_strained(&table_with(&[0.1, 0.6, 0.3])), Some(1));
        assert_eq!(
            most_strained(&table_with(&[0.17, 0.17, 0.17])),
            Some(0),
            "a whole squad back from its first expedition ties, and the tie must be total"
        );
        assert_eq!(
            most_strained(&SquadKnowledge::default()),
            None,
            "a rested squad has nobody to see — not slot 0 by default"
        );
    }

    /// A session gives strain back and touches nothing else.
    #[test]
    fn a_session_spends_strain_and_leaves_what_they_know_alone() {
        let mut k = Knowledge::default();
        k.learn(Subject::BearCopies, Claim::Lethal, Provenance::Firsthand, 0);
        k.accrue_strain(0.5);

        let before = k.of(Subject::BearCopies, Claim::Lethal);
        assert!(k.relieve_strain(0.2));
        assert!((k.strain - 0.3).abs() < 1e-6);
        assert_eq!(
            k.of(Subject::BearCopies, Claim::Lethal),
            before,
            "the cheap verb must not quietly cost knowledge — that is the other one"
        );

        // Nothing to talk about is REFUSED, not silently a no-op that reads as success.
        let mut rested = Knowledge::default();
        assert!(!rested.relieve_strain(0.2));
    }

    /// **The trade, in one test.** A debrief lowers fear by lowering belief, and the containment
    /// benefit goes with it.
    #[test]
    fn a_deep_debrief_buys_calm_with_certainty() {
        let mut k = Knowledge::default();
        k.learn(Subject::BearCopies, Claim::Lethal, Provenance::Firsthand, 0);
        let scared = k.fear_scale(Subject::BearCopies, 0.4);
        assert!(scared > 1.0, "a firsthand lethal belief must raise fear to begin with");

        assert_eq!(k.deep_debrief(0.3), Some(Subject::BearCopies));
        let calmer = k.fear_scale(Subject::BearCopies, 0.4);
        assert!(
            calmer < scared,
            "the whole point: talking them down must actually lower the fear ({calmer} vs {scared})"
        );

        // Burned all the way down, the belief is GONE rather than faint. Absence is a distinct state
        // from doubt (Fisher, via W3014596384) and an operative talked out of a belief is back to
        // not knowing.
        while k.deep_debrief(0.3).is_some() {}
        assert_eq!(k.of(Subject::BearCopies, Claim::Lethal), None);
        assert_eq!(
            k.fear_scale(Subject::BearCopies, 0.4),
            1.0,
            "and ignorance leaves fear exactly unchanged, never 'cautiously' raised"
        );
    }

    /// A debrief costs a `Containable` belief nothing — it is the *lethal* one being talked down.
    #[test]
    fn a_debrief_does_not_touch_what_makes_containment_legible() {
        let mut k = Knowledge::default();
        k.learn(Subject::BearCopies, Claim::Containable, Provenance::Firsthand, 0);
        assert!(k.can_read_rule(Subject::BearCopies));
        assert_eq!(k.deep_debrief(0.3), None, "there is no lethal belief here to burn");
        assert!(
            k.can_read_rule(Subject::BearCopies),
            "knowing HOW to hold a thing must survive being talked down about how deadly it is"
        );
    }

    /// Strain saturates rather than growing without bound.
    #[test]
    fn strain_saturates_because_the_floor_it_feeds_is_a_fraction_of_a_clamped_drive() {
        let mut k = Knowledge::default();
        for _ in 0..20 {
            k.accrue_strain(0.17);
        }
        assert_eq!(k.strain, 1.0);
    }

    /// The panel never goes blank, and it says which state it is in.
    #[test]
    fn the_readout_names_the_quiet_room_rather_than_showing_nothing() {
        let bindings = crate::input::KeyBindings::default();
        let rows = activities_rows(&SquadKnowledge::default(), &[], &bindings);
        let text = format!("{rows:?}");
        assert!(rows.len() >= 2, "a header alone reads as a broken panel");
        assert!(text.contains("QUIET"), "an empty state must be named: {text}");
    }
}
