//! **One question, asked the same way everywhere.**
//!
//! # Why this module exists
//!
//! The editor grew three prompts independently and each invented its own vocabulary. Measured
//! 2026-08-19, before this:
//!
//! | asked by | agree | refuse | third answer |
//! |---|---|---|---|
//! | `chooser` delete / quit | `Y` | `Esc` | — |
//! | `editor` leaving a dirty map | `S` | `Esc` | `D` discards |
//! | `labels` re-label judged pieces | `Enter` | `Esc` | — |
//!
//! Three questions, three agree keys, and `Enter` meaning "yes" in one place while another
//! deliberately refuses to answer to it. An author cannot learn that; they can only read each
//! prompt every time. Reported at the keyboard: *"our prompts keep prompting with all sorts of
//! different keys ... we need to make these uniform."*
//!
//! **`Y` proceeds and `N` stops, everywhere.** `Esc` also refuses, because backing out is what
//! `Esc` means in every other layer of this editor and a modal that ignored it would be the one
//! place it did not work — but `Esc` is a synonym for `N`, never a third outcome.
//!
//! # It is modal, and centred
//!
//! The old prompts were a line of text in the status bar at the bottom-left, which is where this
//! editor puts *commentary*. A question that blocks progress rendered in the same place, in the
//! same colour, as "baked 82 palette thumbnails" is a question an author walks past. This draws a
//! panel in the middle of the window over a dimmed backdrop, so the question is the only thing to
//! answer.
//!
//! # The mouse answers it too
//!
//! Both buttons are clickable. `docs/ui.md` §4.2 wants everything reachable by mouse reachable by
//! keyboard; this is the same rule read the other way, and it is the rule the chooser broke by
//! being keyboard-*only* rather than keyboard-first.
//!
//! # How a caller uses it
//!
//! Asking and answering are deliberately separate frames, and the answer is addressed:
//!
//! ```ignore
//! // somewhere a destructive verb is pressed
//! confirm.ask(Asked::DeleteEntry, format!("Delete `{name}`?"), "This cannot be undone.", "Delete", "Keep");
//!
//! // in that feature's own system, every frame
//! if let Some(true) = confirm.answer(Asked::DeleteEntry) { /* do it */ }
//! ```
//!
//! [`Asked`] is a census of every question the application can raise — the same shape
//! `keys::Action` takes, and for the same reason: a question that is not in the list cannot be
//! asked, so the audit is a `match` rather than a search.

use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;

use crate::chrome::{DIM, Severity, TEXT};

/// **Every question this application can raise.**
///
/// A census rather than a free-form string, so "which prompts exist" is answerable by reading one
/// enum, and so the answer can be routed back to exactly the feature that asked without that
/// feature and this module sharing anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    /// The chooser is about to delete the highlighted kit or map.
    DeleteEntry,
    /// The chooser is about to close the application.
    QuitApp,
    /// The editor is leaving a map with unsaved edits.
    LeaveMap,
    /// A labelling walk found pieces in scope that already carry labels.
    RelabelJudged,
}

impl Asked {
    /// **Does saying yes throw work away?** The only thing the affirmative button's severity
    /// depends on, and the only place this application decides what red means (`chrome::DANGER`:
    /// *"destructive, and only destructive"*).
    ///
    /// Every question used to render its agreeing button filled `DANGER`, whichever question it
    /// was — so `Quit emerge-mapper?`, whose own body says *"anything already saved stays
    /// saved"*, was as red as deleting a kit directory. That is how an author learns to ignore red.
    fn destructive(self) -> bool {
        match self {
            // The whole kit directory, or the map file.
            Asked::DeleteEntry => true,
            // **Yes overwrites labels a human confirmed** with what a model proposes. Nothing is
            // deleted from disk, and judgement is work: the affirmative here is `All 732`, which
            // is the one keystroke in the labelling walk that cannot be got back.
            Asked::RelabelJudged => true,
            // Quitting keeps everything saved; leaving a map saves it on the way out. Both are
            // ordinary, and the affirmative is simply the obvious thing to do.
            Asked::QuitApp | Asked::LeaveMap => false,
        }
    }
}

/// The question on screen, if there is one.
struct Question {
    of: Asked,
    title: String,
    body: String,
    yes: String,
    no: String,
}

/// **The one prompt, and the one answer.**
///
/// `answer` is read by the feature that asked and cleared by the read, so an answer is delivered
/// exactly once — a question answered `yes` twice would run a destructive verb twice, which is the
/// failure the chooser's own `Pending` was built to prevent.
#[derive(Resource, Default)]
pub struct Confirm {
    question: Option<Question>,
    answer: Option<(Asked, bool)>,
}

impl Confirm {
    /// **Raise a question.** A second `ask` while one is up is refused rather than stacked: two
    /// modals cannot both be the only thing to answer, and the one underneath would be answered by
    /// a keystroke aimed at the one on top.
    pub fn ask(
        &mut self,
        of: Asked,
        title: impl Into<String>,
        body: impl Into<String>,
        yes: impl Into<String>,
        no: impl Into<String>,
    ) {
        if self.question.is_some() {
            return;
        }
        self.question = Some(Question {
            of,
            title: title.into(),
            body: body.into(),
            yes: yes.into(),
            no: no.into(),
        });
    }

    /// Is this question the one on screen?
    pub fn asking(&self, of: Asked) -> bool {
        self.question.as_ref().is_some_and(|q| q.of == of)
    }

    /// Is any question on screen? Callers gate their own keyboard on this, the way they used to
    /// gate on their own `leaving`/`ask` flag.
    pub fn is_open(&self) -> bool {
        self.question.is_some()
    }

    /// **Take this question's answer, once.** `Some(true)` agreed, `Some(false)` refused.
    pub fn answer(&mut self, of: Asked) -> Option<bool> {
        match self.answer {
            Some((asked, yes)) if asked == of => {
                self.answer = None;
                Some(yes)
            }
            _ => None,
        }
    }

    /// Answer whatever is up. Used by the keys and the buttons, and by nothing else.
    fn settle(&mut self, yes: bool) {
        if let Some(q) = self.question.take() {
            self.answer = Some((q.of, yes));
        }
    }
}

/// **Marks the modal's scrim layer** — the thing `paint` shows and hides, and the thing that eats a
/// click aimed at the list behind the question. [`crate::chrome::modal_card`] puts this bundle on
/// the layer it spawns and hands back the card.
#[derive(Component)]
struct ConfirmRoot;

/// The two answers, as clickable buttons.
///
/// **The word inside is not marked**, because [`crate::chrome::button`] spawns it: a bundle cannot
/// mark somebody else's child, so `paint` reaches the label through this entity's `Children`. That
/// is the price of the shared builder and it is worth paying — the button's box, its five states
/// and its severity ink all come from one place now, where this module used to state them itself.
#[derive(Component, Clone, Copy)]
struct ConfirmButton(bool);

#[derive(Component)]
struct ConfirmTitle;
#[derive(Component)]
struct ConfirmBody;

pub struct ConfirmPlugin;

impl Plugin for ConfirmPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Confirm>()
            // **Spawned on entering either screen**, not once at startup: `screen::scene_roots`
            // sweeps every parentless UI root on a screen change, and a modal that survived the
            // sweep once would be despawned by it the next time. The guide card learned this the
            // expensive way (`screen.rs`); rather than add a second exclusion, this simply belongs
            // to whichever screen is up.
            .add_systems(
                OnEnter(crate::screen::Screen::Menu),
                spawn.after(crate::chrome::FrameSystems),
            )
            .add_systems(
                OnEnter(crate::screen::Screen::Editor),
                spawn.after(crate::chrome::FrameSystems),
            )
            // **Before `Phase::Act`**, so the frame a question is answered is not also a frame in
            // which the answer key does its ordinary job. `Y` is not bound in the editor's census
            // today, but `N` is `RotateMeshX` on the Meshes tab — so without this ordering,
            // refusing a prompt would also turn the focused mesh a quarter turn.
            .add_systems(
                Update,
                (answer_by_key, paint).before(crate::keys::Phase::Act),
            )
            .add_observer(answer_by_click);
    }
}

/// **`Y` proceeds, `N` stops, `Esc` is a synonym for `N`.**
///
/// Read raw rather than through `keys::just_pressed`: this is not a tab's verb and it must answer
/// identically on the menu, where the editor's key census does not exist at all.
fn answer_by_key(keyboard: Res<ButtonInput<KeyCode>>, mut confirm: ResMut<Confirm>) {
    if confirm.question.is_none() {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyY) {
        confirm.settle(true);
    } else if keyboard.just_pressed(KeyCode::KeyN) || keyboard.just_pressed(KeyCode::Escape) {
        confirm.settle(false);
    }
}

/// The same two answers, by pointer.
fn answer_by_click(
    click: On<Pointer<Click>>,
    buttons: Query<&ConfirmButton>,
    mut confirm: ResMut<Confirm>,
) {
    let Ok(button) = buttons.get(click.entity) else {
        return;
    };
    confirm.settle(button.0);
}

/// Build the card once per screen, hidden. `paint` fills it in and shows it.
///
/// **The scrim, the z-order, the card and its padding are [`crate::chrome::modal_card`]'s now.**
/// This module used to state all four itself, and the audit (F14) found the three modals
/// disagreeing about every one of them: this one at `GlobalZIndex(900)` with a border and
/// `padding: 20`, the token prompt at 400 with neither, and the name box with a fourth scrim
/// written inline. `modal_card` supplies the `SCRIM`, the `GlobalZIndex(MODAL_Z)`, the centring,
/// the `OVERLAY_BG` card with its border and radius, and `MODAL_PAD` — and it starts the layer
/// `Display::None`, because all three modals are built on entering a screen and shown on demand.
///
/// **What stays here is behaviour.** [`ConfirmRoot`] and the `TabGroup` go on the layer, so
/// `paint`'s show/hide is unchanged and `Tab` still walks the two answers. The layer keeps the
/// default `Pickable` rather than `Pickable::IGNORE`, and that is the whole reason this is modal
/// rather than merely centred: a click that misses the card must not land on the list underneath
/// and move the selection the question is about.
fn spawn(mut commands: Commands) {
    crate::chrome::modal_card(&mut commands, (ConfirmRoot, TabGroup::new(0)))
        .with_children(|panel| {
            panel.spawn((
                Text::new(String::new()),
                // **`TEXT`, not `ACCENT`** — the 2026-09-03 decision D6. Amber is a value being
                // changed right now, and a question's title is not one; `chrome::title` moved for
                // the same reason on the same day.
                TextColor(TEXT),
                crate::chrome::font(crate::chrome::text::TITLE),
                ConfirmTitle,
            ));
            panel.spawn((
                Text::new(String::new()),
                TextColor(TEXT),
                crate::chrome::font(crate::chrome::text::BODY),
                ConfirmBody,
            ));
            panel
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(crate::chrome::GAP_ROW),
                    ..default()
                })
                .with_children(|row| {
                    // **Refuse on the left, agree on the right**, and severity is what tells them
                    // apart. The pair used to be a grey box and a `DANGER`-filled box — a filled
                    // red button for every question, including the one whose body says nothing is
                    // lost. Now the affirmative is [`Severity::Primary`] (*the* thing to do here)
                    // or [`Severity::Destructive`] when the question throws work away, the
                    // refusal is always [`Severity::Plain`], and `paint` repaints the affirmative's
                    // ink when the question changes — see [`Asked::destructive`].
                    //
                    // The card's own `MODAL_PAD` and `row_gap: GAP_GROUP` separate this row from
                    // the body above it, which is what the hand-rolled `margin.top: 4` was for.
                    for (yes, severity) in
                        [(false, Severity::Plain), (true, Severity::Primary)]
                    {
                        crate::chrome::button(row, ConfirmButton(yes), "", severity);
                    }
                });
            panel.spawn((
                Text::new("Y proceeds  ·  N stops".to_owned()),
                TextColor(DIM),
                crate::chrome::font(crate::chrome::text::HINT),
            ));
        });
}

/// Show, hide and fill the card. Guarded, because it runs every frame.
///
/// **Nothing here paints a fill any more.** It used to write each button's `BackgroundColor` from
/// its own `Hovered` — one of the six places this editor answered a pointer by hand — and
/// `chrome::style_list_rows` now gives every `chrome::button` the same five states (rest, hover,
/// pressed, selected, disabled) off the `RowRest` the builder carries. Two systems writing one
/// `BackgroundColor` would fight, and the shared one is the one that knows about a press.
#[allow(clippy::type_complexity)]
fn paint(
    confirm: Res<Confirm>,
    mut roots: Query<&mut Node, With<ConfirmRoot>>,
    mut titles: Query<&mut Text, (With<ConfirmTitle>, Without<ConfirmBody>)>,
    mut bodies: Query<&mut Text, (With<ConfirmBody>, Without<ConfirmTitle>)>,
    buttons: Query<(&ConfirmButton, &Children)>,
    // The words inside the buttons. Filtered off the title and the body so the three `Text`
    // queries are disjoint — the hint line matches too and is simply never reached, because this
    // only ever looks at a button's own children.
    mut words: Query<(&mut Text, &mut TextColor), (Without<ConfirmTitle>, Without<ConfirmBody>)>,
) {
    let open = confirm.question.is_some();
    for mut node in &mut roots {
        let want = if open { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
    let Some(q) = confirm.question.as_ref() else {
        return;
    };
    for mut t in &mut titles {
        if t.0 != q.title {
            t.0 = q.title.clone();
        }
    }
    for mut t in &mut bodies {
        if t.0 != q.body {
            t.0 = q.body.clone();
        }
    }
    // **The severity travels with the question, not with the button.** One card serves all four
    // questions, and `Asked::DeleteEntry` is destructive where `Asked::QuitApp` is not — so the
    // affirmative's ink is repainted here rather than chosen once at spawn. `Severity::ink` is
    // asked for it, because the Primary→amber / Destructive→red mapping belongs to `chrome` and
    // restating it here is the drift that module exists to stop.
    let affirmative = if q.of.destructive() {
        Severity::Destructive
    } else {
        Severity::Primary
    };
    for (button, children) in &buttons {
        let (label, ink) = if button.0 {
            (&q.yes, affirmative.ink())
        } else {
            (&q.no, Severity::Plain.ink())
        };
        for child in children.iter() {
            let Ok((mut text, mut colour)) = words.get_mut(child) else {
                continue;
            };
            if &text.0 != label {
                text.0 = label.clone();
            }
            if colour.0 != ink {
                colour.0 = ink;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **An answer is delivered once, and only to the question that asked.**
    #[test]
    fn an_answer_is_addressed_and_taken_once() {
        let mut c = Confirm::default();
        c.ask(Asked::DeleteEntry, "Delete?", "gone for good", "Delete", "Keep");
        assert!(c.is_open() && c.asking(Asked::DeleteEntry));

        c.settle(true);
        assert_eq!(
            c.answer(Asked::LeaveMap),
            None,
            "another feature must not be able to read an answer meant for the one that asked"
        );
        assert_eq!(c.answer(Asked::DeleteEntry), Some(true));
        assert_eq!(
            c.answer(Asked::DeleteEntry),
            None,
            "taken once — a destructive verb read twice would run twice, which is what `Pending` \
             exists to stop"
        );
        assert!(!c.is_open(), "settling takes the question down");
    }

    /// **A second question does not stack on the first.**
    #[test]
    fn one_question_at_a_time() {
        let mut c = Confirm::default();
        c.ask(Asked::DeleteEntry, "first", "", "yes", "no");
        c.ask(Asked::QuitApp, "second", "", "yes", "no");
        assert!(
            c.asking(Asked::DeleteEntry) && !c.asking(Asked::QuitApp),
            "the first question owns the screen until it is answered; two modals cannot both be \
             the only thing to answer, and the one underneath would be answered by a keystroke \
             aimed at the one on top"
        );
    }
}
