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

use crate::chrome::{ACCENT, BAR_EDGE, DANGER, DIM, PANEL_BG, ROW_BG, ROW_HOVER, SCRIM, TEXT};

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

/// Marks the backdrop that dims the application behind the panel.
#[derive(Component)]
struct ConfirmRoot;

/// The two answers, as clickable buttons.
#[derive(Component, Clone, Copy)]
struct ConfirmButton(bool);

#[derive(Component)]
struct ConfirmTitle;
#[derive(Component)]
struct ConfirmBody;
#[derive(Component)]
struct ConfirmLabel(bool);

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

/// Build the panel once per screen, hidden. `paint` fills it in and shows it.
fn spawn(mut commands: Commands) {
    commands
        .spawn((
            ConfirmRoot,
            Node {
                // It covers the whole window, over every dock and band, which is what makes the
                // question the only thing on screen to answer — see [`SCRIM`] just below.
                // PLACES-ITSELF-OK: a modal is not a panel; `chrome::Frame` owns where panels go.
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            // **The backdrop eats the click**, which is what makes this modal rather than merely
            // centred: without it a click that missed the panel would land on the list underneath
            // and move a selection the question is about.
            BackgroundColor(SCRIM),
            GlobalZIndex(900),
            TabGroup::new(0),
        ))
        .with_children(|p| {
            p.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    min_width: Val::Px(360.0),
                    max_width: Val::Px(560.0),
                    padding: UiRect::all(Val::Px(20.0)),
                    row_gap: Val::Px(12.0),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(PANEL_BG),
                BorderColor::all(BAR_EDGE),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new(String::new()),
                    TextColor(ACCENT),
                    TextFont::from_font_size(crate::chrome::text::TITLE),
                    ConfirmTitle,
                ));
                panel.spawn((
                    Text::new(String::new()),
                    TextColor(TEXT),
                    TextFont::from_font_size(crate::chrome::text::BODY),
                    ConfirmBody,
                ));
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(10.0),
                        margin: UiRect::top(Val::Px(4.0)),
                        ..default()
                    })
                    .with_children(|row| {
                        // **Refuse on the left, agree on the right**, and the agreeing one is the
                        // only one coloured: an author scanning for "the one that does the thing"
                        // should find it without reading, and the danger colour is what says the
                        // thing is destructive.
                        for (yes, colour) in [(false, ROW_BG), (true, DANGER)] {
                            row.spawn((
                                bevy::ui_widgets::Button,
                                bevy::picking::hover::Hovered::default(),
                                ConfirmButton(yes),
                                Node {
                                    padding: UiRect::axes(Val::Px(14.0), Val::Px(7.0)),
                                    border: UiRect::all(Val::Px(1.0)),
                                    ..default()
                                },
                                BackgroundColor(colour),
                                BorderColor::all(BAR_EDGE),
                            ))
                            .with_children(|b| {
                                b.spawn((
                                    Text::new(String::new()),
                                    TextColor(TEXT),
                                    TextFont::from_font_size(crate::chrome::text::BODY),
                                    ConfirmLabel(yes),
                                ));
                            });
                        }
                    });
                panel.spawn((
                    Text::new("Y proceeds  ·  N stops".to_owned()),
                    TextColor(DIM),
                    TextFont::from_font_size(crate::chrome::text::HINT),
                ));
            });
        });
}

/// Show, hide and fill the panel. Guarded, because it runs every frame.
#[allow(clippy::type_complexity)]
fn paint(
    confirm: Res<Confirm>,
    mut roots: Query<&mut Node, With<ConfirmRoot>>,
    mut titles: Query<&mut Text, (With<ConfirmTitle>, Without<ConfirmBody>)>,
    mut bodies: Query<&mut Text, (With<ConfirmBody>, Without<ConfirmTitle>)>,
    mut labels: Query<
        (&ConfirmLabel, &mut Text),
        (Without<ConfirmTitle>, Without<ConfirmBody>),
    >,
    mut hovers: Query<
        (&ConfirmButton, &bevy::picking::hover::Hovered, &mut BackgroundColor),
    >,
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
    for (which, mut t) in &mut labels {
        let want = if which.0 { &q.yes } else { &q.no };
        if &t.0 != want {
            t.0 = want.clone();
        }
    }
    for (button, hovered, mut bg) in &mut hovers {
        let base = if button.0 { DANGER } else { ROW_BG };
        let want = if hovered.0 { ROW_HOVER } else { base };
        if bg.0 != want {
            bg.0 = want;
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
