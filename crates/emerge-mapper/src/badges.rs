//! **Every live key, drawn on the thing it acts on, while the shortcut key is held.**
//!
//! # Why this replaced a table
//!
//! `K` used to raise a scrim and a centred two-column list of every chord live in the tab. It was
//! complete, it was generated from the census, and it did not work: reading it means carrying a
//! phrase back to something on screen and hoping you matched the right one. Two verbs that had been
//! bound the whole time — `R` turns a member, `Shift+Delete` empties the tile — were reported
//! *missing* over two sessions, and both were on the list, collapsed onto one row reading
//! `"turn / remove this / Shift: empty the tile"`
//! (`docs/2026-08-15-usability-handoff.md` §2).
//!
//! So the chord goes on the control. Malacria, Bailly, Harrison, Cockburn & Gutwin 2013, *Promoting
//! Hotkey Use through Rehearsal with ExposeHK* (`10.1145/2470654.2470735`) is the measured form of
//! this, and its numbers are the reason the design is a modifier-held overlay rather than anything
//! cleverer: **94%** of selections made by hotkey against 35% for tooltips (Study 1), and **99%**
//! with every menu posted at once (Study 2).
//!
//! # The clutter objection, and why this draws everything at once
//!
//! Study 2 drew **72 badges simultaneously** — six menus, twelve items each — and measured 99%
//! hotkey use, a *lower* error rate than pointing (2.4% vs 5.9%), and the lowest or joint-lowest
//! NASA-TLX in every category including mental demand. Their own words on the worry:
//!
//! > *"Although this design risks giving an initial impression of visual clutter, studies suggest
//! > that similar methods of parallel presentation can improve pointer-based selection performance
//! > and reduce visual search times because **rapid eye saccades can replace comparatively slow
//! > pointer-based manipulation**."*
//!
//! The reframe that makes it true: **you do not search the badges.** You look at the control you
//! already half-know and read the one badge on it. So what has to be capped is how many badges must
//! be *read at one anchor* (`keys::tests::no_home_carries_more_than_it_can_show`, eight), not how
//! many are drawn. This editor's worst case on screen is twenty-one, against their seventy-two.
//!
//! And it is one tier, deliberately. ExposeHK beat Office's Alt-key group-then-member ribbon by
//! **36%**, for two structural reasons: hierarchy forces a multi-level selection, and Alt-keys are
//! *unstable* — the same letter means different things in different modes. Miller, Denkov & Omanson
//! 2011 measured the chunking failure underneath that. A two-stage reveal here would buy a smaller
//! picture and pay for it in the one currency this editor cannot spend.
//!
//! # Two anchors, and no third
//!
//! [`keys::Home`] is the census's answer to *where*, and it is a field of every binding because a
//! verb with nowhere to be drawn is a verb that vanishes from the only surface announcing it:
//!
//! - [`keys::Home::Control`] — on the control it acts through, found by [`chrome::Control`].
//! - [`keys::Home::Legend`] — in a column over empty ground, **with its description beside its
//!   chord**, for a verb that acts on neither. `Esc` backs out and there is nothing on screen it
//!   backs out *of*; drawing it on something would be a claim about that thing. The legend is the one
//!   place a badge carries prose, because it is the one place there is nothing under it to read.
//!
//! # What did not change, and must not
//!
//! **Every key stays live while `K` is down.** `editor::sense_context` never suppressed actions for
//! the old overlay and does not now, so holding `K` and pressing `G` generates — the novice motion
//! and the expert motion are the same motion. That is Kurtenbach's principle of rehearsal, quoted in
//! ExposeHK: *"guidance should be a physical rehearsal of the way an expert would issue a command"*,
//! with the corollary that teaching a hotkey through pointing means *"users rehearse pointing, not
//! hotkey use."* [`flash_live_badges`] is what makes the coincidence visible, and
//! `a_key_still_fires_while_k_is_held` is what stops it being quietly lost.

use bevy::prelude::*;
use bevy::ui::{ComputedNode, Outline, UiGlobalTransform};

use crate::chrome::{self, ACCENT, GAP_TIGHT, KEY, PANEL_BG, VEIL};
use crate::keys::{self, Home};

/// How far a badge sits from the thing it names — one gap, so a cluster reads as belonging to its
/// anchor rather than floating near it.
const REACH: f32 = 4.0;

/// How wide a cluster of bare chords grows before it wraps. Badges within a cluster are ordinary
/// flow children of a wrapping row, so this is the only number deciding a cluster's shape — and two
/// badges at one anchor cannot overlap by construction.
const CLUSTER_W: f32 = 132.0;

/// How wide a legend's descriptions may run before they wrap — a **cap**, not a width.
///
/// The wrapper hugs its words and wraps only past this, so a legend of five short globals is a
/// narrow block and only a long line — the move row's *"move / Shift: clone / M: keep as a
/// composition"* — pays in height. It was a fixed width once, and the fixed block (this cap plus a
/// fixed chord column) measured wider than any free ground the populated Meshes tab has: the legend
/// fell back to its corner and was buried, with every test green. A block that hugs is a block that
/// fits.
const DOES_COL: f32 = 138.0;

/// **The same column on a badge that stands on its own control.**
///
/// Narrower than the legend's, and it has to be: the legend has the viewport's bottom to itself,
/// while a control's cluster shares that ground with every other control's and with the legend. The
/// piece list's five labelled verbs at the legend's width reached halfway across the viewport and
/// clipped both. Narrower wraps more and stays put.
const CONTROL_DOES_COL: f32 = 90.0;

/// **A leader line's thickness, in logical pixels.** One: it is a connector, not a shape, and the
/// badge-border ink ([`KEY`]) at a hairline reads against both the veil and a panel without
/// competing with either.
const LEAD_THICK: f32 = 1.0;

/// One glyph's advance at [`chrome::text::BODY`], logical pixels — FiraMono is the only face in
/// the build and it is monospace, so `chars * BODY_CHAR_W` is exact, not an estimate.
/// `compose::LABEL_CHAR_W` is the same measurement made once before, at the same size.
///
/// It sizes the legend's chord column to the longest chord *this* legend actually holds, so the
/// descriptions line up down the block (`docs/ui.md` §3.1: a panel is rows, not strings) without a
/// constant wide enough for the fattest chord the census has ever had — which is the difference
/// between a legend that fits the populated Meshes tab's free ground and one that measured wider
/// than any of it.
const BODY_CHAR_W: f32 = 6.6;

/// **Nothing renders `keys::rows`.**
///
/// A constant rather than a comment, so `keys::tests::the_row_ceiling_has_no_rendering_left_to_guard`
/// can name it: the twelve-row ceiling existed for a centred table of every chord, this module
/// replaced that table with badges, and a ceiling on a rendering nobody renders is a guard that reads
/// as coverage it does not have. Anything that starts drawing a key *list* again should set this
/// false and bring the ceiling back with it.
pub const RENDERS_NO_ROWS: bool = true;

/// **The layer everything is drawn into.** Carries [`VEIL`] as its own background, so the interface
/// stands back and the badges — its children — draw over it. One node, not two: a UI parent paints
/// its background before its children, which is exactly the stacking wanted here.
#[derive(Component)]
pub struct BadgeLayer;

/// **One anchor's badges.** Positioned by [`place_badges`]; its children are laid out by flex.
#[derive(Component)]
pub struct BadgeCluster(pub Home);

/// **One rendered badge, and the actions it stands for.**
///
/// The actions rather than the chord text, because a chord is a *rendering* and asking "is
/// `Shift+Cmd+Z` held" of a string would be a second parser for something [`keys::pressed`] already
/// answers. (This was `chrome::CensusRow`; the overlay it lit is gone, the reason it exists is not.)
#[derive(Component)]
pub struct Badge(pub Vec<keys::Action>);

/// **What a badge looks like when its key is up** — fill, then border.
///
/// Carried rather than recomputed, because there are two shapes: a chord standing on a control is its
/// own opaque box, and a legend row is a row inside the legend's box and has neither fill nor border
/// of its own. One lighting system serves both, and a system that had to ask which kind it was
/// looking at would be the second place that decides what a badge looks like.
#[derive(Component)]
pub struct BadgeRest(pub Color, pub Color);

/// **The three segments of a cluster's leader** — the reach out of the anchor, the run down the
/// corridor, the landing into the box. Spawned among the layer's children *before* every cluster so
/// a line paints under any box it meets; laid out by [`place_badges`]; the run and the landing stay
/// hidden whenever the box sits level with its anchor and the reach alone is the line.
#[derive(Component)]
pub struct Lead(pub [Entity; 3]);

/// One segment of a leader line — a hairline [`Node`] in the badge-border ink.
#[derive(Component)]
pub struct LeadSeg;

/// **Which tab's badges are up, in which [`keys::Stance`], and against which controls** — so the
/// layer is rebuilt when the answer changes and not on every frame the key is held.
///
/// **The controls belong in the key** for the reason the stance does. A verb is drawn on the thing
/// it acts through, and a thing that is not on screen sends its verb to the legend instead — so a
/// selection appearing in the detail pane changes where five badges live. Keyed on tab and stance
/// alone, they would keep the places they had at the moment the key went down.
///
/// **The stance belongs in this key, not only in the render.** Without it the layer would draw the
/// stance it opened in and then keep it: pick a piece up with the badges on screen and the arrows
/// change job while their badge does not, which is the exact failure this whole change is about,
/// reintroduced one layer up.
///
/// A `Resource` rather than a `Local`, because `screen::close_the_door` sweeps this layer's entities
/// on the way out and a `Local` would survive that — the stale key would suppress the first rebuild
/// and the badges would simply never appear on the next kit.
#[derive(Resource, Default, PartialEq, Eq)]
pub struct ShowingFor(Option<(crate::tiles::Mode, keys::Stance, Vec<keys::ControlId>)>);

pub struct BadgePlugin;

impl Plugin for BadgePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ShowingFor>()
            // **After the frame.** `spawn_layer` reads `Res<Frame>` for the node it parents to, and
            // in Bevy 0.19 a missing `Res<T>` panics its system rather than skipping it.
            .add_systems(
                OnEnter(crate::screen::Screen::Editor),
                spawn_layer.after(chrome::FrameSystems),
            )
            // Chained: a cluster spawned this frame is placed this frame and lit this frame, rather
            // than two frames later — which for a tap is the difference between a readout and a
            // flicker.
            .add_systems(
                Update,
                (rebuild_badges, place_badges, flash_live_badges, light_anchored_controls)
                    .chain()
                    .in_set(keys::Phase::Act)
                    .run_if(in_state(crate::screen::Screen::Editor)),
            );
    }
}

/// The layer, spawned once and hidden until the key is down.
fn spawn_layer(mut commands: Commands, frame: Res<chrome::Frame>) {
    let layer = commands
        .spawn((
            BadgeLayer,
            // Not a panel — a layer over the whole window, whose children are each placed by
            // projecting a control's rect or a world point. `chrome::Frame` owns where a panel goes;
            // nothing here is in the flow it governs.
            Node {
                // PLACES-ITSELF-OK: a layer, not a panel — see above.
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                display: Display::None,
                ..default()
            },
            // **Per `docs/ui.md` §5 trap 7.** A full-screen container without this eats every world
            // click. It must also never carry `Hovered`: `view::over_ui` and `view::drive` ask "is
            // the pointer on the interface" by looking for any true one, and a layer covering the
            // window would answer yes everywhere — killing zoom and click-to-place with nothing on
            // screen to point at.
            bevy::picking::Pickable::IGNORE,
            // Above the panels (0), the compass (100), the tab strip (101) and the name box (400).
            // Below `confirm`'s 900, deliberately: a question you have to answer outranks a hint
            // about keys.
            GlobalZIndex(500),
            BackgroundColor(VEIL),
        ))
        .id();
    commands.entity(frame.root).add_child(layer);
}

/// **The rect a control occupies, or `None` if it is not laid out at all.**
///
/// `chrome::panel_root`'s hidden form is `Display::None`, which measures zero — that is a whole
/// panel belonging to a tab nobody is on, and a badge aimed at it would point at nothing. That is
/// the only way to be absent, and [`resolve`] sends those verbs to the legend.
///
/// # A row scrolled past the fold used to count as absent, and that was the wrong answer
///
/// It did, for a real reason: taking a scrolled-out row's rect at face value stacked `T F G H`,
/// `Z X V`, `B N O P` and the label verbs on top of each other at the pane's bottom edge, pointing
/// at rows nobody could see. So they were sent to the legend instead, with their descriptions.
///
/// **That fix moved the problem rather than solving it.** With the detail pane scrolled, six groups
/// arrived in the legend at once and it grew to twenty labelled rows — as tall as the viewport,
/// which the piece list's own badges then drew straight through. The arithmetic does not close: a
/// ~262 px legend plus two ~215 px labelled dock clusters do not fit in ~588 px of viewport, and no
/// placement rule makes them.
///
/// So the fold is back to meaning what it means everywhere else in this module — a **bound**, not a
/// verdict. [`anchor`] already answers `within` from it, [`place_badges`] clamps to that, and the
/// stacking pass below is what the original stack-at-the-edge report actually needed. A verb whose
/// row is out of sight is pinned at the edge of the pane it is in, which is where scrolling to it
/// would bring it.
fn laid_out_rect(node: &ComputedNode, tf: &UiGlobalTransform) -> Option<Rect> {
    let size = node.size();
    (size != Vec2::ZERO).then(|| Rect::from_center_size(tf.translation, size))
}

/// The scrolling pane a control sits **inside**, if any. Strict: a node that *is* the `ScrollArea` is
/// a whole control with open ground beside it, not a row within one.
fn fold_of(
    entity: Entity,
    parents: &Query<&ChildOf>,
    folds: &Query<&bevy::ui_widgets::ScrollArea>,
    rects: &Query<(&ComputedNode, &UiGlobalTransform)>,
) -> Option<Rect> {
    let mut at = parents.get(entity).ok().map(|p| p.parent());
    while let Some(e) = at {
        if folds.get(e).is_ok() {
            let (node, tf) = rects.get(e).ok()?;
            return (node.size() != Vec2::ZERO)
                .then(|| Rect::from_center_size(tf.translation, node.size()));
        }
        at = parents.get(e).ok().map(|p| p.parent());
    }
    None
}

/// **Where a verb is actually drawn**, given what is on screen to draw it on.
///
/// A chord is drawn bare *because the thing under it says what it does*. A chord with nothing under
/// it has to say so itself, and that is what the legend is — so a control that is not laid out sends
/// its verbs there, with their descriptions, rather than leaving them nowhere. One rule, evaluated
/// from one fact, and `pub` so the test that checks the outcome cannot drift into a second copy of it.
pub fn resolve(home: Home, on_screen: &[keys::ControlId]) -> Home {
    match home {
        Home::Control(id) if !on_screen.contains(&id) => Home::Legend,
        other => other,
    }
}

/// Show the layer while the key is down, and rebuild its clusters when what is live changes.
fn rebuild_badges(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<keys::Live>,
    mode: Res<crate::tiles::Mode>,
    mut showing: ResMut<ShowingFor>,
    mut layers: Query<(Entity, &mut Node), With<BadgeLayer>>,
    clusters: Query<Entity, With<BadgeCluster>>,
    // **No fold queries here any more.** Whether a control is laid out is a fact about the control;
    // where its badge may go is a fact about the pane, and that is `place_badges`'s question.
    controls: Query<(Entity, &chrome::Control, &ComputedNode, &UiGlobalTransform)>,
) {
    let want = keys::pressed(&keyboard, *live, keys::Action::Shortcuts);
    for (_, mut node) in &mut layers {
        let display = if want { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    // **What is on screen to be drawn on**, in `ControlId::ALL` order so the key is stable and a
    // query's arbitrary order cannot make it rebuild for nothing. See [`laid_out_rect`] for what
    // "on screen" has to mean — only a hidden panel is absent; a row scrolled past its fold keeps
    // its badge and is pinned at the edge of the pane holding it.
    let on_screen: Vec<keys::ControlId> = keys::ControlId::ALL
        .into_iter()
        .filter(|id| {
            controls
                .iter()
                .any(|(_, c, node, tf)| c.0 == *id && laid_out_rect(node, tf).is_some())
        })
        .collect();
    let key = want.then(|| (*mode, live.1, on_screen.clone()));
    if showing.0 == key {
        return;
    }
    showing.0 = key.clone();
    for cluster in &clusters {
        commands.entity(cluster).despawn();
    }
    let Some((tab, stance, on_screen)) = key else { return };

    // **This tab's verbs, then the frame's** — the order the old list used, so anyone who learned it
    // finds the same things in the same order. `badges` returns declaration order and never sorts:
    // a badge's *position* is half of what is being learned here.
    //
    // **A verb whose control is not on screen joins the legend.** One rule, not two paths: a chord is
    // drawn bare *because the thing under it says what it does*, so a chord with nothing under it has
    // to say so itself — which is what the legend is. The detail pane's rows only exist once a piece
    // is picked, and before that `I`, `M`, `T F G H [ ]`, `Z X V`, `B N O P` and `L …` would otherwise
    // have nowhere to be at all.
    //
    // **And a verb the door cannot honour is not drawn at all.** The slot keys are `Context::Global`
    // because the strip is, but the strip's chips come from `Door::tabs()` — so on the Map and Rigs
    // doors `2` and `3` were badges for panels that do not exist. `Action::fires_on_a_door_of` is the
    // one place that answers it, and `tiles::tab_shortcuts` derives the same fact from the same
    // mapping. The door is read off the tab rather than taken as a resource: `Door::showing` is
    // total, and `ShowingFor` already keys on the tab, so the layer rebuilds when the door changes.
    let panels = crate::tiles::Door::showing(tab).tabs().len();
    let live_badges: Vec<keys::Badge> = keys::badges(tab.context(), stance)
        .into_iter()
        .chain(keys::badges(keys::Context::Global, stance))
        .filter_map(|b| b.on_a_door_of(panels))
        .map(|mut b| {
            b.home = resolve(b.home, &on_screen);
            b
        })
        .collect();

    // One cluster per distinct home, sorted so the legend spawns first, then the controls. Bevy
    // paints siblings in spawn order, so where two anchors genuinely overlap — a list's
    // badge against the legend beside it — the more specific one draws on top.
    let mut homes: Vec<Home> = Vec::new();
    for badge in &live_badges {
        if !homes.contains(&badge.home) {
            homes.push(badge.home);
        }
    }
    homes.sort_by_key(|h| paint_order(*h));

    for (layer, _) in &mut layers {
        // **Leaders under boxes.** Siblings paint in spawn order, so every segment is spawned
        // before any cluster: a line that meets a box passes under its face and the box stays
        // readable, which is the reading order the whole overlay exists for.
        let mut leads: Vec<(Home, [Entity; 3])> = Vec::new();
        for home in homes.iter().filter(|h| matches!(h, Home::Control(_))) {
            let mut three = [Entity::PLACEHOLDER; 3];
            commands.entity(layer).with_children(|p| {
                for seg in &mut three {
                    *seg = p
                        .spawn((
                            LeadSeg,
                            Node {
                                // PLACES-ITSELF-OK: a leader segment, laid against its anchor and
                                // its box by `place_badges` — same standing as the clusters.
                                position_type: PositionType::Absolute,
                                ..default()
                            },
                            BackgroundColor(KEY),
                            Visibility::Hidden,
                            bevy::picking::Pickable::IGNORE,
                        ))
                        .id();
                }
            });
            leads.push((*home, three));
        }
        for home in &homes {
            commands.entity(layer).with_children(|p| {
                // **A legend is a column; everything else is a wrapping row of bare chords.** The
                // difference is what each is for: a chord sitting on a control is read *with* the
                // control, so it wants to be small and out of the words; a legend is read on its own
                // and wants one verb per line — the shape a key list always should have had, and
                // never did on a collapsed row.
                let legend = *home == Home::Legend;
                // **One shape for every dock**: a column of rows, the chord on the left and what it
                // does on the right. A band has no column in it, so a badge there is the bare chord
                // beside a control whose own words are already the verb.
                //
                // Asked for outright, after a sweep found a dock carrying a bare letter on one row, a
                // labelled column beside a list, and a keypad drawn as a cross, all at the same time.
                let labelled = match *home {
                    Home::Legend => true,
                    Home::Control(id) => !id.in_a_band(),
                };
                // The chord column the legend's rows share: the longest chord it holds, measured
                // exactly (monospace), so descriptions align without a census-wide constant.
                let chord_col = live_badges
                    .iter()
                    .filter(|b| b.home == Home::Legend)
                    .map(|b| b.chord.chars().count())
                    .max()
                    .unwrap_or(0) as f32
                    * BODY_CHAR_W
                    + 1.0;
                let mut spawned = p.spawn((
                    BadgeCluster(*home),
                    // A cluster stands where its anchor is, which flow has no opinion about —
                    // `place_badges` computes it from a rect or a projection every frame.
                    Node {
                        // PLACES-ITSELF-OK: placed against its anchor — see above.
                        position_type: PositionType::Absolute,
                        flex_direction: if labelled {
                            FlexDirection::Column
                        } else {
                            FlexDirection::Row
                        },
                        flex_wrap: if labelled { FlexWrap::NoWrap } else { FlexWrap::Wrap },
                        // **A legend's rows are all one width**, so it reads as a block rather than
                        // as a ragged stack of boxes — `Stretch` takes the widest and gives it to
                        // every row. A cluster of bare chords wants the opposite: each box hugs its
                        // own chord, because there they are labels on things and not a table.
                        align_items: if legend {
                            AlignItems::Stretch
                        } else {
                            AlignItems::Start
                        },
                        max_width: if labelled { Val::Auto } else { Val::Px(CLUSTER_W) },
                        // **The legend is ONE box**, and its rows are rows. A border per row drew a
                        // ladder of rules between them, so the whole thing read as a table with
                        // gridlines rather than as one object parked out of the way. A cluster of
                        // bare chords carries no box at all — there, each badge is its own.
                        padding: if legend { chrome::CHIP_PAD } else { UiRect::ZERO },
                        border: if legend {
                            UiRect::all(Val::Px(1.0))
                        } else {
                            UiRect::ZERO
                        },
                        column_gap: Val::Px(GAP_TIGHT),
                        row_gap: Val::Px(GAP_TIGHT),
                        ..default()
                    },
                    BackgroundColor(if legend { PANEL_BG } else { Color::NONE }),
                    BorderColor::all(if legend { KEY } else { Color::NONE }),
                    // Hidden, not `Display::None`: a node that is not displayed is never laid out,
                    // so it could never acquire the size `place_badges` needs to position it. A
                    // visibility-hidden UI node does occupy layout, which is the difference.
                    Visibility::Hidden,
                    bevy::picking::Pickable::IGNORE,
                ));
                if let Some((_, three)) = leads.iter().find(|(h, _)| h == home) {
                    spawned.insert(Lead(*three));
                }
                spawned.with_children(|c| {
                    for badge in live_badges.iter().filter(|b| b.home == *home) {
                        one_badge(c, badge, legend, labelled, chord_col);
                    }
                });
            });
        }
    }
}

/// **The rect a [`Home`] is drawn against**, in surface pixels.
///
/// [`ComputedNode`] + [`UiGlobalTransform`] are in physical surface pixels, so there is exactly one
/// conversion, at the write, and it is by [`UiScale`].
///
/// `None` is *"there is no honest answer"* and the cluster stays hidden. It is never repositioned to
/// somewhere else — a home that quietly moved is precisely what would hide a control that has
/// stopped being drawn.
struct Anchored {
    at: Rect,
    /// **The scrolling pane that clips this control, if one does.** It no longer bounds the box —
    /// boxes pack in a rail and never bury one another — it clamps the **leader's anchor end**: a
    /// row scrolled past the fold is pointed at from the pane edge nearest it, which is where
    /// scrolling would bring it back. The earlier design pinned the *box* there instead, and on a
    /// real kit a scrolled pane stacked boxes on one edge until the deepest were unreadable.
    fold: Option<Rect>,
    /// **Is this control in one of the frame's fixed-height bands?**
    ///
    /// The chrome bar, the door strip and the status band are twenty-six logical pixels of chrome
    /// with no slack; a badge top-aligned to a control there hangs below it, and reported from the
    /// keyboard, *"there's not enough room to offset them below."* So a banded badge is **centred
    /// on its control** — level with the thing it names — and joins no rail: a band is its own
    /// ground, and its badges are bare chords that fit it.
    banded: bool,
}

fn anchor(
    home: Home,
    frame: &chrome::Frame,
    rects: &Query<(&ComputedNode, &UiGlobalTransform)>,
    controls: &Query<(Entity, &chrome::Control, &ComputedNode, &UiGlobalTransform)>,
    parents: &Query<&ChildOf>,
    folds: &Query<&bevy::ui_widgets::ScrollArea>,
) -> Option<Anchored> {
    fn rect_of(node: &ComputedNode, tf: &UiGlobalTransform) -> Option<Rect> {
        let size = node.size();
        // A `Display::None` node lays out to nothing, which is also this module's visibility test.
        (size != Vec2::ZERO).then(|| Rect::from_center_size(tf.translation, size))
    }
    match home {
        Home::Control(id) => {
            let mut found = controls
                .iter()
                .filter(|(_, c, ..)| c.0 == id)
                .filter_map(|(e, _, node, tf)| laid_out_rect(node, tf).map(|r| (e, r)));
            let first = found.next()?;
            if found.next().is_some() {
                // Two visible nodes claiming one id is a bug rather than a tie to break, and
                // choosing between them would hide it. `every_home_a_live_binding_names_is_on_screen`
                // is the guard; this is the report when it has already shipped.
                warn_once!("two visible controls claim {id:?}; its badges are not drawn");
                return None;
            }
            let (entity, at) = first;
            let fold = fold_of(entity, parents, folds, rects);
            // Asked of the frame, not of a height: `chrome::Frame` names these three, so a band that
            // grows or shrinks cannot make this answer stale the way a pixel threshold would.
            let bands = [frame.chrome_bar, frame.door_strip, frame.status];
            let banded = std::iter::successors(Some(entity), |e| {
                parents.get(*e).ok().map(|p| p.parent())
            })
            .any(|e| bands.contains(&e));
            Some(Anchored { at, fold, banded })
        }
        Home::Legend => {
            // **The hole the world is drawn through**, because that is the only ground in this
            // window that belongs to nothing — the anchor here is only the preferred corner's rect;
            // [`settle_legend`] is what actually finds the ground.
            let (node, tf) = rects.get(frame.viewport).ok()?;
            rect_of(node, tf).map(|at| Anchored { at, fold: None, banded: false })
        }
    }
}

/// **Where a home sits in the PAINT order** — the legend first, so that where a leader passes the
/// legend's ground the box still draws over the line. Bevy paints siblings in spawn order, so this
/// is a spawn-time question and [`rebuild_badges`] is where it is asked — and the leader segments
/// are spawned before everything here, which is what puts every line under every box.
fn paint_order(home: Home) -> usize {
    match home {
        Home::Legend => 0,
        Home::Control(_) => 1,
    }
}

/// **Where a home sits in the PLACEMENT order**, which is deliberately not [`paint_order`].
///
/// Banded boxes hold their bands, the rails pack next, and the legend — which has a whole stage to
/// stand in — yields to all of them. Within a rail the primary key is the **anchor's own y** (see
/// [`place_badges`]: order preservation is what makes right-angle leaders crossing-free); this
/// index is the tie-break, stated because `Query` iteration order is **not stable across `App`
/// instances** — without a stated key the overlay would lay itself out differently on two runs of
/// the same test.
fn place_order(home: Home) -> (usize, usize) {
    match home {
        Home::Control(id) => (
            0,
            keys::ControlId::ALL
                .iter()
                .position(|c| *c == id)
                .unwrap_or(usize::MAX),
        ),
        Home::Legend => (1, 0),
    }
}

/// Does `a` cover any real ground of `b` — more than a hairline's worth in both directions?
fn covers(a: Rect, b: Rect) -> bool {
    let i = a.intersect(b);
    i.width() > 1.0 && i.height() > 1.0
}

/// **The first free `y` at or below `pref` for a box of `size` at `x`, clear of everything placed.**
///
/// Bounded by `taken.len()`: each step lands this box just past one rect it hit and `y` only grows,
/// so the loop cannot run longer than the list it is dodging — and after stepping past a rect, that
/// rect can never hit again. There is **no give-up arm**. The old placement stopped when its bound
/// ran out and *let the overlap show*, and a real kit on a real window priced that honesty: the
/// legend under the piece list's boxes, four cell rows buried beneath their own neighbours. A box
/// that cannot hug its row now sits further down the same rail, and the leader carries the
/// attachment — that trade is what the leader exists to buy.
fn settle_down(pref: f32, size: Vec2, x: f32, taken: &[Rect], gap: f32) -> f32 {
    let mut y = pref;
    for _ in 0..=taken.len() {
        let me = Rect::from_corners(Vec2::new(x, y), Vec2::new(x, y) + size);
        let Some(hit) = taken.iter().find(|t| covers(**t, me)) else {
            break;
        };
        y = hit.max.y + gap;
    }
    y
}

/// **The legend's ground: the corner column if it can, else the nearest free column to it —
/// never an overlap.**
///
/// The same corner every time is what makes the legend learnable as a *place*
/// ([`keys::Home::Legend`]), so the corner's own column is always tried first and the search never
/// settles further from it than the placed boxes force. Candidate columns are taken from the
/// **edges of the boxes themselves** — one just left of each placed rect — walked rightmost-first;
/// a greedy "shift past the leftmost blocker" was tried and it leapt clean over the free ground
/// between the two rails on the populated Tiles tab, straight to a dead column, and fell back onto
/// the piece list's boxes. Fekete & Plaisant's excentric labels (`10.1145/302979.303148`) state
/// the rule this search implements: a callout lives in free space, and free space is found, not
/// hoped for. Bounded: at most `taken + 1` columns, each climbed past at most `taken` rects.
fn settle_legend(size: Vec2, stage: Rect, taken: &[Rect], gap: f32) -> Vec2 {
    let corner = (stage.max - size - Vec2::splat(gap * 3.0)).max(stage.min);
    let mut columns: Vec<f32> = std::iter::once(corner.x)
        .chain(taken.iter().map(|t| t.min.x - size.x - gap))
        .filter(|x| *x >= stage.min.x && *x <= corner.x)
        .collect();
    columns.sort_by(|a, b| b.total_cmp(a));
    columns.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    for x in columns {
        let mut y = corner.y;
        for _ in 0..=taken.len() {
            let me = Rect::from_corners(Vec2::new(x, y), Vec2::new(x, y) + size);
            let Some(top) = taken
                .iter()
                .filter(|t| covers(**t, me))
                .map(|t| t.min.y)
                .reduce(f32::min)
            else {
                return Vec2::new(x, y);
            };
            let up = top - size.y - gap;
            if up < stage.min.y {
                break;
            }
            y = up;
        }
    }
    // No free column at all — the corner, and the overlap ratchet is what says whether this state
    // is ever actually reached.
    corner
}

/// **Place every cluster — rail-packed, never overlapping, each tied to its anchor by a
/// right-angle leader — and reveal it once it has a size.**
///
/// **The model is boundary labeling** (Bekos, Kaufmann, Symvonis & Wolff 2007,
/// `10.1016/j.comgeo.2006.05.003`): boxes stand on one straight rail beside the dock that owns
/// their anchors, stacked **in their anchors' order** — the condition under which right-angle
/// leaders cannot cross one another. Fekete & Plaisant's excentric labels
/// (`10.1145/302979.303148`) are the other parent: a callout on free ground, tied by a line, beats
/// a box squeezed onto ground it does not fit. Two vertical runs may coincide on the corridor —
/// two lines becoming one line is legible; two lines crossing is not, and the sort is what forbids
/// the second. (Marschner & Shirley's chapter on diagram aesthetics says the same two words this
/// paragraph keeps using: minimize crossings and bends.)
///
/// A leader is three hairline segments in the badge-border ink: the **reach** out of the anchor's
/// stage-facing edge, the **run** down the corridor between dock and rail — ground no box can
/// stand on — and the **landing** into the box. A box level with its anchor draws the reach alone,
/// which is the common case and reads as the old adjacency.
///
/// Runs every frame the layer is up, because the panels move: a list scrolls, a pane grows a row,
/// the docks resize with the window. A badge that only moved when the census changed would sit
/// still through exactly the part an author is watching.
fn place_badges(
    frame: Res<chrome::Frame>,
    ui_scale: Res<UiScale>,
    rects: Query<(&ComputedNode, &UiGlobalTransform)>,
    controls: Query<(Entity, &chrome::Control, &ComputedNode, &UiGlobalTransform)>,
    parents: Query<&ChildOf>,
    folds: Query<&bevy::ui_widgets::ScrollArea>,
    layers: Query<Entity, With<BadgeLayer>>,
    mut clusters: Query<(
        &BadgeCluster,
        &ComputedNode,
        &mut Node,
        &mut Visibility,
        Option<&Lead>,
    )>,
    mut segs: Query<(&mut Node, &mut Visibility), (With<LeadSeg>, Without<BadgeCluster>)>,
) {
    let Some(layer) = layers.iter().next() else {
        return;
    };
    let Some(window) = rects
        .get(layer)
        .ok()
        .map(|(node, tf)| Rect::from_center_size(tf.translation, node.size()))
        .filter(|r| r.size() != Vec2::ZERO)
    else {
        return;
    };

    // `Val::Px` is multiplied by `UiScale` and everything above is in surface pixels, so this is the
    // one conversion — `compose::place_labels` paid for putting it anywhere else, and every label it
    // drew landed 20% further from the corner than the point it named. A zero or negative scale is a
    // host misconfiguration rather than a state to render around; guarded so it cannot make a NaN.
    let scale = if ui_scale.0 > 0.0 { ui_scale.0 } else { 1.0 };
    let reach = REACH * scale;
    let thick = (LEAD_THICK * scale).max(1.0);

    // **The stage: the hole the world is drawn through**, intersected with the window because the
    // viewport is a flex item and on a window too narrow for both docks it overflows the frame.
    // Boxes and the legend live here and nowhere else — a box over a panel would cover the words
    // that identify what some other badge points at, and the docks are not veiled ground.
    let stage = rects
        .get(frame.viewport)
        .ok()
        .map(|(node, tf)| Rect::from_center_size(tf.translation, node.size()))
        .filter(|r| r.size() != Vec2::ZERO)
        .map(|r| r.intersect(window))
        .unwrap_or(window);

    let mut items: Vec<_> = clusters.iter_mut().collect();
    items.sort_by_key(|(cluster, ..)| place_order(cluster.0));
    let anchors: Vec<Option<Anchored>> = items
        .iter()
        .map(|(cluster, node, ..)| {
            (node.size() != Vec2::ZERO)
                .then(|| anchor(cluster.0, &frame, &rects, &controls, &parents, &folds))
                .flatten()
        })
        .collect();

    // **The rails: every un-banded control box column-aligns just past the widest anchor on its
    // side.** One straight rail per side reads as one thing, and it is what gives every leader's
    // vertical run a corridor — the strip between dock edge and rail — where no box can stand.
    let mid = window.center().x;
    let side_of = |a: &Anchored| a.at.center().x <= mid;
    let mut left_edge = f32::MIN;
    let mut right_edge = f32::MAX;
    for (i, a) in anchors.iter().enumerate() {
        let (Some(a), (cluster, ..)) = (a, &items[i]) else {
            continue;
        };
        if matches!(cluster.0, Home::Control(_)) && !a.banded {
            if side_of(a) {
                left_edge = left_edge.max(a.at.max.x);
            } else {
                right_edge = right_edge.min(a.at.min.x);
            }
        }
    }

    #[derive(Clone, Copy)]
    struct Plan {
        pos: Option<Vec2>,
        lead: [Option<Rect>; 3],
    }
    let mut banded_plans: Vec<Plan> = (0..items.len())
        .map(|_| Plan { pos: None, lead: [None; 3] })
        .collect();
    let mut taken: Vec<Rect> = Vec::new();

    // **The elbow between an anchor point and a box**, as up to three hairline rects.
    let elbow = |a: Vec2, corridor_x: f32, box_edge_x: f32, box_center_y: f32| -> [Option<Rect>; 3] {
        let h = |x0: f32, x1: f32, y: f32| {
            Rect::from_corners(
                Vec2::new(x0.min(x1), y - thick * 0.5),
                Vec2::new(x0.max(x1), y + thick * 0.5),
            )
        };
        if (a.y - box_center_y).abs() <= thick {
            [Some(h(a.x, box_edge_x, a.y)), None, None]
        } else {
            let v = Rect::from_corners(
                Vec2::new(corridor_x - thick * 0.5, a.y.min(box_center_y)),
                Vec2::new(corridor_x + thick * 0.5, a.y.max(box_center_y)),
            );
            [
                Some(h(a.x, corridor_x, a.y)),
                Some(v),
                Some(h(corridor_x, box_edge_x, box_center_y)),
            ]
        }
    };

    // ── Banded boxes first: their ground is their band, beside their control as before. ──────────
    for (i, a) in anchors.iter().enumerate() {
        let (Some(a), (cluster, node, ..)) = (a, &items[i]) else {
            continue;
        };
        if !(matches!(cluster.0, Home::Control(_)) && a.banded) {
            continue;
        }
        let size = node.size();
        let leading = a.at.min.x - size.x - reach;
        let x = if leading >= window.min.x {
            leading
        } else {
            a.at.max.x + reach
        };
        let y = a.at.center().y - size.y * 0.5;
        let pos = Vec2::new(x, y);
        let me = Rect::from_corners(pos, pos + size);
        banded_plans[i].pos = Some(pos);
        // The stub that says "this box is that control's": anchor edge to box edge, level.
        let (a_x, box_edge) = if x >= a.at.max.x {
            (a.at.max.x, x)
        } else {
            (a.at.min.x, x + size.x)
        };
        banded_plans[i].lead = elbow(Vec2::new(a_x, a.at.center().y), a_x, box_edge, a.at.center().y);
        taken.push(me);
        for seg in banded_plans[i].lead.iter().flatten() {
            taken.push(*seg);
        }
    }

    // ── The rails and the legend, twice if needed. ───────────────────────────────────────────────
    //
    // **Comfort first, compactness if the legend cannot stand.** A rail box prefers to sit level
    // with its anchor, and on most tabs that comfort costs nothing. On the populated Tiles tab it
    // cost everything: the slack above the first box was exactly the ground the legend needed, and
    // no free rectangle remained anywhere in the stage — measured 22 px short in the best band.
    // So the packing runs once with preferences honoured; if the legend's search then ends in a
    // colliding corner, it runs once more with every rail packed tight from the stage's top —
    // association survives in the leaders, which is what the leaders are for — and the legend
    // search runs again against the ground that freed. Two passes at most, both deterministic.
    let banded_taken = taken.clone();
    let rail_of = |left: bool| -> Vec<usize> {
        let mut rail: Vec<usize> = (0..items.len())
            .filter(|i| {
                matches!(items[*i].0 .0, Home::Control(_))
                    && anchors[*i].as_ref().is_some_and(|a| !a.banded && side_of(a) == left)
            })
            .collect();
        rail.sort_by(|x, y| {
            let (ax, ay) = (
                anchors[*x].as_ref().map(|a| a.at.center().y).unwrap_or(0.0),
                anchors[*y].as_ref().map(|a| a.at.center().y).unwrap_or(0.0),
            );
            ax.total_cmp(&ay)
                .then(place_order(items[*x].0 .0).cmp(&place_order(items[*y].0 .0)))
        });
        rail
    };

    let attempt = |compact: bool| -> (Vec<Plan>, Vec<Rect>) {
        let mut plans: Vec<Plan> = (0..items.len())
            .map(|_| Plan { pos: None, lead: [None; 3] })
            .collect();
        // Banded boxes were placed once above; copy their plans and ground into this attempt.
        for (i, p) in banded_plans.iter().enumerate() {
            if p.pos.is_some() {
                plans[i] = Plan { pos: p.pos, lead: p.lead };
            }
        }
        let mut taken = banded_taken.clone();
        for left in [true, false] {
            let mut floor = stage.min.y;
            for i in rail_of(left) {
                let a = anchors[i].as_ref().unwrap_or_else(|| unreachable!());
                if let Some(f) = a.fold {
                    let seen = a.at.intersect(f);
                    if seen.width() <= 0.0 || seen.height() <= 0.0 {
                        continue;
                    }
                }
                let size = items[i].1.size();
                let (x, corridor_x, a_x) = if left {
                    (left_edge + reach * 2.0, left_edge + reach, a.at.max.x)
                } else {
                    (right_edge - reach * 2.0 - size.x, right_edge - reach, a.at.min.x)
                };
                let pref = if compact {
                    floor
                } else {
                    a.at
                        .min
                        .y
                        .clamp(stage.min.y, (stage.max.y - size.y).max(stage.min.y))
                        .max(floor)
                };
                let y = settle_down(pref, size, x, &taken, reach)
                    .clamp(stage.min.y, (stage.max.y - size.y).max(stage.min.y));
                let pos = Vec2::new(x, y);
                plans[i].pos = Some(pos);
                let box_edge = if left { x } else { x + size.x };
                // **The leader's anchor end: the nearest point on the anchor's edge, not its
                // centre.** A list is taller than its badge, and centre-attachment drew a run the
                // full half-height of the palette — long enough that the legend, refusing to cover
                // it, gave up its own corner. Nearest-point is the standard callout attachment and
                // collapses the common case back to a bare stub. Clamped into the fold when the
                // row is beyond it — pointed at from where scrolling would bring it back.
                let a_y = {
                    let near = (y + size.y * 0.5)
                        .clamp(a.at.min.y + thick, (a.at.max.y - thick).max(a.at.min.y));
                    match a.fold {
                        Some(f) => near.clamp(f.min.y + thick, (f.max.y - thick).max(f.min.y)),
                        None => near,
                    }
                };
                plans[i].lead = elbow(Vec2::new(a_x, a_y), corridor_x, box_edge, y + size.y * 0.5);
                taken.push(Rect::from_corners(pos, pos + size));
                for seg in plans[i].lead.iter().flatten() {
                    taken.push(*seg);
                }
                floor = y + size.y + reach;
            }
        }
        // The legend last: it has a whole stage to stand in, so it yields to everything.
        for (i, a) in anchors.iter().enumerate() {
            let ((cluster, node, ..), Some(_)) = (&items[i], a) else {
                continue;
            };
            if cluster.0 != Home::Legend {
                continue;
            }
            let size = items[i].1.size();
            let _ = node;
            plans[i].pos = Some(settle_legend(size, stage, &taken, reach));
        }
        (plans, taken)
    };

    let (mut plans, mut taken_out) = attempt(false);
    let legend_collides = |plans: &[Plan], taken: &[Rect]| -> bool {
        (0..items.len()).any(|i| {
            items[i].0 .0 == Home::Legend
                && plans[i].pos.is_some_and(|at| {
                    let size = items[i].1.size();
                    let me = Rect::from_corners(at, at + size);
                    taken.iter().any(|t| covers(*t, me))
                })
        })
    };
    if legend_collides(&plans, &taken_out) {
        let again = attempt(true);
        // Keep the compact answer only if it actually houses the legend — otherwise comfort
        // stands and the overlap ratchet is what reports the screen that defeated both.
        if !legend_collides(&again.0, &again.1) {
            plans = again.0;
            taken_out = again.1;
        }
    }
    let _ = taken_out;

    // ── Write everything, gated. ─────────────────────────────────────────────────────────────────
    for (i, (_, _, style, visibility, lead)) in items.iter_mut().enumerate() {
        let show = match plans[i].pos {
            Some(at) => {
                let (left, top) = (Val::Px(at.x / scale), Val::Px(at.y / scale));
                if style.left != left {
                    style.left = left;
                }
                if style.top != top {
                    style.top = top;
                }
                Visibility::Inherited
            }
            None => Visibility::Hidden,
        };
        if **visibility != show {
            **visibility = show;
        }
        let Some(lead) = lead else { continue };
        for (k, entity) in lead.0.iter().enumerate() {
            let Ok((mut node, mut vis)) = segs.get_mut(*entity) else {
                continue;
            };
            let want = if plans[i].pos.is_none() { None } else { plans[i].lead[k] };
            match want {
                Some(r) => {
                    let (l, t) = (Val::Px(r.min.x / scale), Val::Px(r.min.y / scale));
                    let (w, h) = (
                        Val::Px((r.width() / scale).max(LEAD_THICK)),
                        Val::Px((r.height() / scale).max(LEAD_THICK)),
                    );
                    if node.left != l {
                        node.left = l;
                    }
                    if node.top != t {
                        node.top = t;
                    }
                    if node.width != w {
                        node.width = w;
                    }
                    if node.height != h {
                        node.height = h;
                    }
                    if *vis != Visibility::Inherited {
                        *vis = Visibility::Inherited;
                    }
                }
                None => {
                    if *vis != Visibility::Hidden {
                        *vis = Visibility::Hidden;
                    }
                }
            }
        }
    }
}

/// **The novice path and the expert path are the same motion — this is what says so.**
///
/// Holding `K` and pressing `G` already *works*: `editor::sense_context` never suppressed actions
/// while the overlay is up, so every key on screen is live the whole time. What was missing, before
/// its ancestor `chrome::flash_live_rows`, is that nothing said it had happened — and an unremarked
/// coincidence is not a bridge.
///
/// Cockburn, Gutwin, Scarr & Malacria 2014 (`10.1145/2659796`) is the reason it matters: offering a
/// fast path beside a slow one **does not work on its own** — users plateau on the slow one, because
/// no single moment hurts enough to justify switching. The fix they name is a *bridge* where the
/// novice route rehearses the expert route, Kurtenbach & Buxton's marking menus being the canonical
/// one. `docs/ui.md` §3.5 states the practical form: *"a control group that binds invisibly is a
/// control group nobody binds twice — the readout has to show it happened."*
///
/// It is a strictly better bridge here than it was as a row in a list, and that is the whole change
/// in one system: the thing that lights under your finger is now sitting **on the control**, which
/// is ExposeHK's Goal 3 rather than an approximation of it.
///
/// **Held rather than flashed.** A one-frame highlight on `just_pressed` is invisible at 60 Hz for
/// exactly the key you were looking at. This uses [`keys::pressed`], so a badge stays lit as long as
/// the key is down — which for a repeating key is also its cadence, drawn.
fn flash_live_badges(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<keys::Live>,
    mut badges: Query<(&Badge, &BadgeRest, &mut BackgroundColor, &mut BorderColor)>,
) {
    for (badge, rest, mut bg, mut border) in &mut badges {
        let lit = badge.0.iter().any(|a| keys::pressed(&keyboard, *live, *a));
        let (fill, edge) = if lit {
            (chrome::ROW_SELECTED, ACCENT)
        } else {
            (rest.0, rest.1)
        };
        // Written through a compare: both are change-detected, and touching every badge every frame
        // would mark the whole layer dirty sixty times a second for one lit chord.
        if bg.0 != fill {
            bg.0 = fill;
        }
        if border.top != edge {
            *border = BorderColor::all(edge);
        }
    }
}

/// **A control that carries badges says so itself while the overlay is up.**
///
/// The badge stands *beside* its control — a paid-for placement: drawn on it, it covered exactly
/// the words that identify the control. Beside is honest, and it is also deniable: in a dock of
/// stacked rows, "whose chord is this" is answered by proximity alone. So while `K` is held, the
/// control that owns a cluster carries a one-pixel [`ACCENT`] outline — ownership drawn on the
/// owner, with nothing covered.
///
/// `want` derives from the spawned clusters rather than from the census, so door-trimming and
/// [`resolve`]'s fallback are already applied: a control whose verbs fell to the legend does not
/// light, and when the key is up there are no clusters, so everything rests. The outline toggles by
/// **colour** rather than by insert/remove — `Outline`'s own doc recommends `Color::NONE` for
/// exactly this — and it draws outside the node, so lighting a control cannot reflow it. Rest is
/// `NONE` by definition rather than a carried state: no other system writes `Outline`, which is
/// what spares this one a `BadgeRest` twin.
fn light_anchored_controls(
    mut commands: Commands,
    clusters: Query<&BadgeCluster>,
    mut controls: Query<(Entity, &chrome::Control, Option<&mut Outline>)>,
) {
    let want: Vec<keys::ControlId> = clusters
        .iter()
        .filter_map(|c| match c.0 {
            Home::Control(id) => Some(id),
            Home::Legend => None,
        })
        .collect();
    for (entity, control, outline) in &mut controls {
        let target = if want.contains(&control.0) {
            ACCENT
        } else {
            Color::NONE
        };
        match outline {
            // Compare-and-set: an unconditional write every frame is the shape
            // `tests/no_system_writes_every_frame.rs` polices out of this crate.
            Some(mut on) => {
                if on.color != target {
                    on.color = target;
                }
            }
            None if target != Color::NONE => {
                commands
                    .entity(entity)
                    .insert(Outline::new(Val::Px(1.0), Val::Px(1.0), target));
            }
            None => {}
        }
    }
}

/// **One badge, wherever it is going.** The pad and the ordinary flow spawn the same thing; a second
/// spelling of a badge is the drift `chrome.rs` exists to stop, one module along.
fn one_badge(
    c: &mut ChildSpawnerCommands,
    badge: &keys::Badge,
    legend: bool,
    labelled: bool,
    chord_col: f32,
) {
    c.spawn((
        Badge(badge.actions.clone()),
        Node {
            padding: chrome::CHIP_PAD,
            // A legend row is inside the legend's box and needs no box of its
            // own; a chord standing on a control is its own object and does.
            border: if legend {
                UiRect::ZERO
            } else {
                UiRect::all(Val::Px(1.0))
            },
            flex_direction: FlexDirection::Row,
            // **Start, not Center.** A wrapped description is two lines tall and
            // its chord is one; centring floated the chord in the middle of the
            // pair, so a column of chords stopped being a column.
            align_items: AlignItems::Start,
            column_gap: Val::Px(chrome::GAP_ROW),
            ..default()
        },
        // Opaque, and that is what makes a standalone badge legible over both
        // grounds: against the bright viewport the near-black fill is maximum
        // contrast, and against a panel — where the fill matches the ground —
        // the border is what separates it. The border is not decoration.
        BackgroundColor(if legend { Color::NONE } else { PANEL_BG }),
        BorderColor::all(if legend { Color::NONE } else { KEY }),
        BadgeRest(
            if legend { Color::NONE } else { PANEL_BG },
            if legend { Color::NONE } else { KEY },
        ),
        bevy::picking::Pickable::IGNORE,
    ))
    .with_children(|b| {
        b.spawn((
            Node {
                // The chord holds a column so the descriptions line up down the legend —
                // `docs/ui.md` §3.1's argument that a panel is rows and not strings, applied to the
                // last thing in this editor that was still a string. Only inside the legend's one
                // box: a standalone badge is its own object and hugs its chord.
                min_width: if legend { Val::Px(chord_col) } else { Val::Auto },
                flex_shrink: 0.0,
                ..default()
            },
            Text::new(badge.chord.clone()),
            TextColor(KEY),
            // The chord is the one thing a badge exists to have read, so it gets the reading
            // size — the descriptions stay at `HINT`, quieter on both axes.
            TextFont::from_font_size(chrome::text::BODY),
            // A chord with a space in it is one token to a reader and two to a
            // line-breaker.
            TextLayout::new(Justify::Left, LineBreak::NoWrap),
            bevy::picking::Pickable::IGNORE,
        ));
        if labelled {
            // **The width lives on a wrapper, not on the text node.**
            //
            // A `Text` measures itself, and a `max_width` on the same node it
            // measures on is applied *after* — so it reported one line's height,
            // the row was built that tall, and the second line drew below the box
            // and through the row beneath it. Reported from the keyboard: *"the
            // box is too small for the text, so the text just slips under it
            // where it can't be seen."* Constrained from outside, the measure
            // runs against the width it will actually get.
            b.spawn(Node {
                // A column, so the block has a width you can predict. Capping the
                // *cluster* instead was tried and is the wrong lever:
                // `align_items: Stretch` plus a shrinkable row let flex take the
                // words down to nothing rather than wrap them.
                // A cap, not a width: the wrapper hugs its words and wraps only past this, so a
                // short description costs exactly what it measures. The fixed width before it made
                // every box as wide as the longest description the census allows.
                max_width: Val::Px(if legend { DOES_COL } else { CONTROL_DOES_COL }),
                ..default()
            })
            .with_children(|w| {
                w.spawn((
    Text::new(badge.does.to_owned()),
    // Quieter than the chord: the chord is what the eye scans
    // for, the words are what tell it whether to stop.
    TextColor(chrome::DIM),
    TextFont::from_font_size(chrome::text::HINT),
    TextLayout::new(Justify::Left, LineBreak::WordBoundary),
    bevy::picking::Pickable::IGNORE,
                ));
            });
        }
    });

}
