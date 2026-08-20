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
//! # Three anchors, and no fourth
//!
//! [`keys::Home`] is the census's answer to *where*, and it is a field of every binding because a
//! verb with nowhere to be drawn is a verb that vanishes from the only surface announcing it:
//!
//! - [`keys::Home::Control`] — on the control it acts through, found by [`chrome::Control`].
//! - [`keys::Home::Legend`] — in a column over empty ground, **with its description beside its
//!   chord**, for a verb that acts on neither. `Cmd+Z` undoes and there is nothing on screen it
//!   undoes *on*; drawing it on something would be a claim about that thing. The legend is the one
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
use bevy::ui::{ComputedNode, UiGlobalTransform};

use crate::chrome::{self, ACCENT, GAP_TIGHT, KEY, PANEL_BG, VEIL};
use crate::keys::{self, Home};

/// How far a badge sits from the thing it names — one gap, so a cluster reads as belonging to its
/// anchor rather than floating near it.
const REACH: f32 = 4.0;

/// How wide a cluster of bare chords grows before it wraps. Badges within a cluster are ordinary
/// flow children of a wrapping row, so this is the only number deciding a cluster's shape — and two
/// badges at one anchor cannot overlap by construction.
const CLUSTER_W: f32 = 132.0;

/// How wide a legend's descriptions may run before they wrap.
///
/// With [`CHORD_COL`] beside it this fixes the block's width, which is what keeps it inside the
/// viewport: the longest line in the census — the label verbs' *"suggest / all-or-hold / apply /
/// discard / clear all"* — is otherwise wider than the ground the legend stands on, and it pushed the
/// whole block over the piece list.
///
/// The pair has to add up to less than the viewport, and the viewport is what is left of the window
/// once both docks have taken theirs. Widening either is a decision about that, not a free choice.
const DOES_COL: f32 = 138.0;

/// **The same column on a badge that stands on its own control.**
///
/// Narrower than the legend's, and it has to be: the legend has the viewport's bottom to itself,
/// while a control's cluster shares that ground with every other control's and with the legend. The
/// piece list's five labelled verbs at the legend's width reached halfway across the viewport and
/// clipped both. Narrower wraps more and stays put.
const CONTROL_DOES_COL: f32 = 96.0;

/// **The strip at the viewport's leading edge that belongs to the left dock's badges.**
///
/// Wide enough for the chord column of a dock's badge list plus its gap. The legend is kept out of
/// it, which is what stops the two drawing through each other.
const GUTTER: f32 = 88.0;

/// The column a legend's chords hold, so its descriptions line up under each other rather than
/// stepping in and out with the width of the chord above.
///
/// Wide enough for the longest chord this census can render — the labeler's `L, Shift+L, U, Y,
/// Shift+Y`, twenty-five characters. It was set for `Cmd+Z, Shift+Cmd+Z` and that was too narrow: a
/// chord past the column pushes its description right, widening the whole block, and the block is
/// already about as wide as the viewport it has to sit in.
///
/// A `min_width` rather than a `width` all the same, so a longer chord some day pushes rather than
/// clips: the same choice, for the same reason, the old two-column key list made and wrote five lines
/// about.
const CHORD_COL: f32 = 140.0;

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

/// **Which tab's badges are up, in which [`keys::Stance`], and against which controls** — so the
/// layer is rebuilt when the answer changes and not on every frame the key is held.
///
/// **The controls and the subject belong in the key** for the reason the stance does. A verb is drawn
/// on the thing it acts through, and a thing that is not on screen sends its verb to the legend
/// instead — so a selection appearing in the detail pane changes where five badges live, and picking
/// a piece up moves the piece-verbs onto it. Keyed on tab and stance alone, they would keep the
/// places they had at the moment the key went down.
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
                (rebuild_badges, place_badges, flash_live_badges)
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

    // One cluster per distinct home, sorted so the legend spawns first, then the controls, then the
    // subject. Bevy paints siblings in spawn order, so where two anchors genuinely overlap — a list's
    // badge against the legend beside it — the more specific one draws on top.
    let mut homes: Vec<Home> = Vec::new();
    for badge in &live_badges {
        if !homes.contains(&badge.home) {
            homes.push(badge.home);
        }
    }
    homes.sort_by_key(|h| paint_order(*h));

    for (layer, _) in &mut layers {
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
                p.spawn((
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
                ))
                .with_children(|c| {
                    for badge in live_badges.iter().filter(|b| b.home == *home) {
                        one_badge(c, badge, legend, labelled);
                    }
                });
            });
        }
    }
}

/// **The rect a [`Home`] is drawn against**, in surface pixels.
///
/// One space for all three anchors, and that is what makes this one mechanism rather than two
/// wearing one name: [`ComputedNode`] + [`UiGlobalTransform`] are in physical surface pixels, and so
/// is `Camera::world_to_viewport` here — `MainCamera` renders into `surface::Surface`'s image, whose
/// `target_scaling_factor()` is 1.0, and the projection already carries the viewport's offset inside
/// the target. So there is exactly one conversion, at the write, and it is by [`UiScale`].
///
/// `None` is *"there is no honest answer"* and the cluster stays hidden. It is never repositioned to
/// somewhere else — a home that quietly moved is precisely what would hide a control that has
/// stopped being drawn.
///
/// It answers **two** rects: where the badge is aimed, and the ground it may not leave. They differ
/// per home and the difference is the point — a control's badge is bounded by the *pane* that holds
/// it, vertically, so a row scrolled below the fold pins its chord at that pane's edge instead of
/// sliding down to the foot of the window, where it reads as belonging to the status band.
struct Anchored {
    at: Rect,
    within: Rect,
    /// **Is this control in one of the frame's fixed-height bands?**
    ///
    /// The chrome bar, the door strip and the status band are twenty-six logical pixels of chrome
    /// with no slack; a badge top-aligned to a control there hangs below it, and reported from the
    /// keyboard, *"there's not enough room to offset them below."* So a banded badge is **centred on
    /// its control** — level with the thing it names, which is what a label beside a button should
    /// be. A dock has vertical room, so a badge there stays level with the *row* it names rather than
    /// with the middle of a pane.
    banded: bool,
}

fn anchor(
    home: Home,
    frame: &chrome::Frame,
    rects: &Query<(&ComputedNode, &UiGlobalTransform)>,
    controls: &Query<(Entity, &chrome::Control, &ComputedNode, &UiGlobalTransform)>,
    parents: &Query<&ChildOf>,
    folds: &Query<&bevy::ui_widgets::ScrollArea>,
    window: Rect,
    stage: Rect,
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
            // **The pane that clips it, if one does.** Horizontally the badge is free inside the
            // window — it lives *beside* the pane, which is the whole point — so only the vertical
            // band comes from the fold.
            let fold = fold_of(entity, parents, folds, rects);
            let within = match fold {
                Some(f) => Rect::from_corners(
                    Vec2::new(window.min.x, f.min.y),
                    Vec2::new(window.max.x, f.max.y),
                )
                .intersect(window),
                None => window,
            };
            // Asked of the frame, not of a height: `chrome::Frame` names these three, so a band that
            // grows or shrinks cannot make this answer stale the way a pixel threshold would.
            let bands = [frame.chrome_bar, frame.door_strip, frame.status];
            let banded = std::iter::successors(Some(entity), |e| {
                parents.get(*e).ok().map(|p| p.parent())
            })
            .any(|e| bands.contains(&e));
            Some(Anchored { at, within, banded })
        }
        Home::Legend => {
            // **The hole the world is drawn through**, because that is the only ground in this
            // window that belongs to nothing. The chrome bar and the status band were tried first,
            // one region each, and both are twenty-six pixels tall: a chord with its description
            // beside it does not fit in either, so the cluster spilled out of the band it was
            // supposed to be tidied into. A legend needs a column, and only the viewport has one.
            let (node, tf) = rects.get(frame.viewport).ok()?;
            rect_of(node, tf).map(|at| Anchored { at, within: stage, banded: false })
        }
    }
}

/// **Where a cluster's top-left corner goes**, given its anchor, its own measured size, and the rect
/// it has to stay inside.
///
/// Three placements, one per kind of anchor, each chosen so a cluster does not land on the thing it
/// is pointing at:
///
/// - **A control** is named from its leading edge, level with it in a band and level with its row in
///   a dock. **The window is not inset for this**, and that was tried: `I` and `M` sit in the panel's
///   own `MARGIN + PAD`, which fits them with about a pixel to spare, so insetting the bound by a
///   margin made them stop fitting and flip out of the panel entirely. A badge one pixel from the
///   window edge reads tight; a badge on the wrong side of its panel reads wrong. — the gutter beside
///   it, where the panel's own
///   `MARGIN` usually leaves room. **Where it does not, it goes to the trailing edge instead**, and
///   that is a correction: clamping it onto the leading edge was tried first and it covered exactly
///   the words that identify the control — `Cmd+O` over `‹ ki`, `1, 2, 3` over `M`, `Cmd+C` over
///   `TILE`. A badge that hides the label it is attached to has undone its own job. Every anchor in
///   the right-hand dock still uses the leading side, so nothing that already read well moved.
/// - **The legend** takes the viewport's **bottom-right** corner, inset. The top-left was tried first
///   and it is the one corner that is *not* free: a control in the left dock puts its badges just
///   past the panel's trailing edge, top-aligned with the pane — so on Meshes and Tiles the detail
///   pane's seven chords landed straight across the legend's own. The legend is nearly as wide as
///   the viewport, so there is no horizontal strip to move it into; the free ground is vertical, and
///   the far corner is the only one no control cluster and no gizmo reaches. The same corner every
///   time is what makes it learnable as a position rather than read as a list.
/// - **A subject** is named off its trailing edge, level with it — the piece stays uncovered and the
///   chords read left-to-right away from it.
///
/// An anchor as wide as the window — the door strip, the chrome bar — has room on neither side, and
/// there the clamp still puts the cluster on an edge. That is the honest last answer: a badge pinned
/// to an edge still names its key, and one drawn off the window does not.
fn spot(home: Home, anchor: Rect, cluster: Vec2, scale: f32, bound: Rect, banded: bool) -> Vec2 {
    // Every constant here is written in logical pixels, the units the rest of this crate states
    // lengths in; everything it is compared against is physical. One multiply, at the top, so a
    // future constant cannot be added in the wrong space.
    let reach = REACH * scale;
    match home {
        Home::Control(_) => {
            let leading = anchor.min.x - cluster.x - reach;
            let x = if leading >= bound.min.x {
                leading
            } else {
                anchor.max.x + reach
            };
            // Level with the control in a band; level with the *row* in a dock, which is what keeps a
            // badge attached to the line it names rather than to the middle of the pane holding it.
            let y = if banded {
                anchor.center().y - cluster.y * 0.5
            } else {
                anchor.min.y
            };
            Vec2::new(x, y)
        }
        // **The far corner, and nothing to dodge in it.** It was raised by the compass's own height
        // for a while, because the gizmo owns the opposite end of this edge — and then the gizmo
        // learned to stand down while the badges are up, which is the better fix and made the
        // clearance dead weight. Dead weight with a consequence: it pushed the legend up into the
        // lattice cursor's cross.
        Home::Legend => anchor.max - cluster - Vec2::splat(reach * 3.0),
    }
}

/// **Where a home sits in the PAINT order** — the legend first, so that where two anchors genuinely
/// overlap the more specific badge draws on top. Bevy paints siblings in spawn order, so this is a
/// spawn-time question and [`rebuild_badges`] is where it is asked.
fn paint_order(home: Home) -> usize {
    match home {
        Home::Legend => 0,
        Home::Control(_) => 1,
    }
}

/// **Where a home sits in the PLACEMENT order**, which is deliberately not [`paint_order`].
///
/// A cluster is placed clear of the ones placed before it, so the order *is* the priority: a badge
/// standing on a control has one right place and the legend has a whole corner, so the controls go
/// first and the legend yields. That is the opposite of the paint order, and the two are not one
/// list read twice — they answer different questions about the same pair.
///
/// Within the controls, `ControlId::ALL` order, because `Query` iteration order is **not stable
/// across `App` instances**: without a stated key, two clusters clamped to one pane edge would
/// stack in whichever order the query happened to yield, and the overlay would lay itself out
/// differently on two runs of the same test.
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

/// **Step a cluster clear of the ones already placed**, without leaving the ground it is bound to.
///
/// Several controls in one pane can clamp to the same edge — a scrolled detail pane puts `I`, `M`,
/// the cursor keys, the cell keys, the mesh keys and the label verbs all at the same `y`. Stacked
/// they are six unreadable boxes in one place, which is the report this whole area started from:
/// *"why is rescan in the middle legend area when there is a UI button for it."*
///
/// `down` is which way the free ground lies, and it follows the anchor: a control's badge is aimed
/// at the top of its row, so it steps down; the legend sits in the bottom corner, so it steps up.
///
/// **Bounded by `taken.len()`**, because each step puts this cluster past exactly one rect it hit
/// and the `y` only moves one way — so the loop cannot run longer than the list it is dodging. When
/// there is no room left it stops and lets the overlap show: a badge pinned somewhere honest beats
/// one pushed off the ground it belongs to.
fn step_clear(taken: &[Rect], mut at: Vec2, size: Vec2, within: Rect, down: bool, gap: f32) -> Vec2 {
    let hi = (within.max - size).max(within.min);
    for _ in 0..taken.len() {
        let me = Rect::from_corners(at, at + size);
        let Some(hit) = taken.iter().find(|t| !t.intersect(me).is_empty()) else {
            break;
        };
        let want = if down {
            hit.max.y + gap
        } else {
            hit.min.y - size.y - gap
        };
        if want < within.min.y || want > hi.y {
            break;
        }
        at.y = want;
    }
    at
}

/// Place every cluster against its anchor, and reveal it once it has a size to place.
///
/// Runs every frame the layer is up, because the panels move: a list scrolls, a pane grows a row, the
/// docks resize with the window. A badge that only moved when the census changed would sit still
/// through exactly the part an author is watching.
fn place_badges(
    frame: Res<chrome::Frame>,
    ui_scale: Res<UiScale>,
    rects: Query<(&ComputedNode, &UiGlobalTransform)>,
    controls: Query<(Entity, &chrome::Control, &ComputedNode, &UiGlobalTransform)>,
    parents: Query<&ChildOf>,
    folds: Query<&bevy::ui_widgets::ScrollArea>,
    layers: Query<Entity, With<BadgeLayer>>,
    mut clusters: Query<(&BadgeCluster, &ComputedNode, &mut Node, &mut Visibility)>,
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

    // **The legend stands in the hole the world is drawn through, so it is bounded by that.** The
    // viewport is already the one answer to "where the world is" — `surface::fit_viewport_to_frame`
    // hands the same rect to the map camera — and intersecting it with the window is what stops a
    // squeezed layout, where the viewport overflows the frame, carrying the legend off the edge.
    let stage = rects
        .get(frame.viewport)
        .ok()
        .map(|(node, tf)| Rect::from_center_size(tf.translation, node.size()))
        .filter(|r| r.size() != Vec2::ZERO)
        // **Intersected with the window, not taken raw.** The viewport is a flex item, and on a
        // window too narrow for both docks it overflows the frame — so clamping to it alone let the
        // legend run off the right edge, which the clamp exists to prevent. The viewport says where
        // these belong; the window says what can be seen; a badge has to satisfy both.
        .map(|r| r.intersect(window))
        // **Minus the strip the left dock's badges land in.** Every badge for a row in that panel
        // sits just outside it, and the legend reaches back far enough to meet them: measured with
        // the lattice cursor's cross drawn *through* the legend, `G` rendering as `G +S` across
        // `Cmd+S  save`. Both were unreadable. [`GUTTER`] is what keeps them apart, and it is why
        // [`DOES_COL`] is as narrow as it is.
        .map(|r| Rect::from_corners(Vec2::new(r.min.x + GUTTER * scale, r.min.y), r.max))
        .filter(|r| r.size().x > 1.0)
        .unwrap_or(window);

    // **One pass, in a stated order, each cluster clear of the last** — see [`place_order`] for why
    // the order is written down rather than taken from the query, and [`step_clear`] for what
    // "clear" costs when the ground runs out.
    let mut items: Vec<_> = clusters.iter_mut().collect();
    items.sort_by_key(|(cluster, ..)| place_order(cluster.0));
    let mut taken: Vec<Rect> = Vec::new();
    for (cluster, node, mut style, mut visibility) in items {
        let size = node.size();
        let placed = (size != Vec2::ZERO)
            .then(|| {
                anchor(cluster.0, &frame, &rects, &controls, &parents, &folds, window, stage)
            })
            .flatten()
            .map(|Anchored { at, within, banded }| {
                // The bound decides which side of a control the badge goes, so `anchor` answers it
                // alongside the rect rather than it being decided in two places.
                let want = spot(cluster.0, at, size, scale, within, banded);
                // Clamped rather than taken down: a badge pinned to an edge still names its key, and
                // a badge that vanished when its anchor drifted off screen would be missing exactly
                // when an author is hunting for it.
                let hi = (within.max - size).max(within.min);
                let want = want.clamp(within.min, hi);
                // Which way the free ground lies is the anchor's own answer: a control's badge is
                // top-aligned with its row, the legend stands in the bottom corner.
                let down = matches!(cluster.0, Home::Control(_));
                let want = step_clear(&taken, want, size, within, down, REACH * scale);
                taken.push(Rect::from_corners(want, want + size));
                (want, within)
            });

        let show = match placed {
            Some((at, _)) => {
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
        if *visibility != show {
            *visibility = show;
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

/// **One badge, wherever it is going.** The pad and the ordinary flow spawn the same thing; a second
/// spelling of a badge is the drift `chrome.rs` exists to stop, one module along.
fn one_badge(c: &mut ChildSpawnerCommands, badge: &keys::Badge, legend: bool, labelled: bool) {
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
                min_width: if legend { Val::Px(CHORD_COL) } else { Val::Auto },
                flex_shrink: 0.0,
                ..default()
            },
            Text::new(badge.chord.clone()),
            TextColor(KEY),
            TextFont::from_font_size(chrome::text::HINT),
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
                width: Val::Px(if legend { DOES_COL } else { CONTROL_DOES_COL }),
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
