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
use bevy::ui::widget::ImageNode;
use bevy::ui::{ComputedNode, UiGlobalTransform};

use crate::chrome::{self, ACCENT, GAP_TIGHT, KEY, PANEL_BG, VEIL};
use crate::keys::{self, Home};

/// **How far a badge stands off the interface** — the gap to its anchor, the corridor its leader
/// runs down, and the clearance the legend keeps from everything.
///
/// [`chrome::GAP_GROUP`], the scale's **between-blocks** step, because a badge and the panel it is
/// pointing at are two blocks. `chrome`'s spacing note states the rule this obeys (van den Berg,
/// Cornelissen & Roerdink 2009, `10.1167/9.4.24`): clutter is *crowding*, and a group reads as a
/// group when its members sit closer to each other than to anything else. So the gap out to the
/// interface has to be the larger one, and [`STACK`] — badge to badge inside one column — the
/// smaller.
///
/// **It was 4 px, and that was one gap doing both jobs.** Every part of a leader was then four
/// pixels long: the reach out of the anchor, the corridor's clearance, the landing back into the
/// box. Three hairlines inside four pixels do not read as a line, they read as a smudge at the
/// panel's edge — reported from the keyboard, *"the lines aren't distinct."*
///
/// Four was the right number when adjacency was the **only** thing tying a badge to its control.
/// It is not any more — [`Lead`] draws the tie — so the association survives a gap that proximity
/// alone could not have afforded.
const REACH: f32 = chrome::GAP_GROUP;

/// **How far apart two leaders' corridors stand.**
///
/// One corridor per rail was the first design, on the argument that *"two vertical runs may coincide
/// on the corridor — two lines becoming one line is legible."* On a real screen it was not: six
/// leaders down one sixteen-pixel strip put every vertical on top of every other and every elbow
/// within a few pixels of the next, so the whole thing read as one bracket with tick marks.
/// Reported from the keyboard: *"if two lines have a ninety degree angle close to each other, we
/// should stagger those apart so that the user can see they are separate lines."*
///
/// So every leader gets a **lane of its own**, and the rail steps out far enough to clear the widest
/// band a side can need. Bekos et al. call this a multi-track boundary labeling; the tracks are what
/// buy the separation, and [`lanes_for`] is what keeps them from buying crossings with it.
const LANE: f32 = chrome::GAP_ROW;

/// **How far one badge sits from the badge above it**, in a column that shares a corridor.
///
/// [`chrome::GAP_ROW`] — the gap *within* a block, deliberately much smaller than [`REACH`]. Boxes
/// on one rail are one object; widening this with the reach would have pulled a rail apart into
/// unrelated boxes at exactly the moment the reach was making it stand clear of the panel.
const STACK: f32 = chrome::GAP_ROW;

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

/// **The resolution the screen's free ground is measured at**, in surface pixels.
///
/// [`FreeGround`] answers "is this rectangle clear" in constant time by asking a grid of cells
/// rather than by scanning two hundred rects, and a cell **any** ink touches is occupied — so a
/// rectangle this says is free is genuinely free, and the exact ratchet
/// (`no_badge_cluster_draws_through_another`) still means what it says. The rounding costs at most
/// one cell of usable ground at each edge, which is why it is smaller than [`REACH`] is large.
///
/// Eight rather than one: at 2560×1406 this is 320 × 176 cells, which is a grid that can be rebuilt
/// from scratch every frame the key is held without anybody noticing.
const CELL: f32 = 8.0;

/// One glyph's advance at [`chrome::text::BODY`] — [`chrome::BODY_CHAR_W`], the one
/// measurement, stated beside the size it belongs to.
///
/// It sizes the legend's chord column to the longest chord *this* legend actually holds, so the
/// descriptions line up down the block (`docs/ui.md` §3.1: a panel is rows, not strings) without a
/// constant wide enough for the fattest chord the census has ever had — which is the difference
/// between a legend that fits the populated Meshes tab's free ground and one that measured wider
/// than any of it.
const BODY_CHAR_W: f32 = chrome::BODY_CHAR_W;

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

/// **Everything on screen a reader needs to see** — the set a badge must not stand on.
///
/// Fekete & Plaisant's excentric labels (`10.1145/302979.303148`) state the rule this implements: a
/// callout lives in free space, and free space is **found**, not hoped for. Until this existed the
/// only thing the overlay measured was its own boxes, so "free" meant "no other badge" and a rail
/// hugging a dock put a box straight through the map — while the left dock's column below a short
/// panel, a whole 560 × 460 px of it on a 2560 × 1406 window, sat unused.
///
/// A node is ink when it is laid out, visible, outside this overlay's own layer, and **paints**: a
/// background with alpha, a border with width and colour, a `Text`, or an `ImageNode`.
///
/// **The interface only.** What the world draws is not in here and is not dodged either: it is
/// faded instead while the key is held (`chrome::WORLD_HELD`), which is why this census has one
/// tier where it used to have two.
///
/// **A [`chrome::Ground`] node paints nothing that counts, and that is the load-bearing exception.**
/// A panel's fill is what its rows stand on, not a thing to read, so subtracting the container while
/// keeping every child in it is what makes a pane's empty middle placeable — the ground beside the
/// rows a badge is naming, which is as close to ExposeHK's third goal as this editor can get. Its
/// border goes with it: a container's frame is part of the container.
///
/// **The layer's own subtree is excluded**, because it paints [`VEIL`] across the whole window
/// ([`spawn_layer`]) and would otherwise mark every pixel on screen occupied.
fn ink(
    layer: Entity,
    painters: &Query<(
        Entity,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
        Option<&BackgroundColor>,
        Option<&BorderColor>,
        Option<&Text>,
        Option<&ImageNode>,
        Option<&chrome::Ground>,
    )>,
    parents: &Query<&ChildOf>,
) -> Vec<Rect> {
    let mut out: Vec<Rect> = Vec::new();
    for (entity, node, tf, seen, bg, border, text, image, ground) in painters.iter() {
        if ground.is_some() || !seen.get() {
            continue;
        }
        let Some(rect) = laid_out_rect(node, tf) else {
            continue;
        };
        let edged = {
            let b = node.border;
            b.min_inset.x + b.min_inset.y + b.max_inset.x + b.max_inset.y > 0.0
                && border.is_some_and(|c| {
                    [c.top, c.right, c.bottom, c.left]
                        .iter()
                        .any(|k| k.alpha() > 0.0)
                })
        };
        // **An empty `Text` paints nothing**, and there are a lot of them: every readout row in this
        // editor holds a value node that is blank until there is a value — `UNDER` with nothing
        // under the cursor, `EDGES` before a check. Counting those as ink would fence off a strip
        // of the pane for a string that is not there, and unfence it the moment it is, which is the
        // grid moving under a reader for no reason they can see.
        let paints = bg.is_some_and(|c| c.0.alpha() > 0.0)
            || text.is_some_and(|t| !t.0.trim().is_empty())
            || image.is_some()
            || edged;
        if !paints {
            continue;
        }
        // The overlay's own children paint over everything by design; they are not ground it has to
        // dodge. Walked rather than filtered by marker, because a badge's `Text` carries none.
        let mine = std::iter::successors(Some(entity), |e| parents.get(*e).ok().map(|p| p.parent()))
            .any(|e| e == layer);
        if !mine {
            out.push(rect);
        }
    }
    out
}

/// **Free ground, quantised — the screen's unused pixels, answerable in constant time.**
///
/// One flag per [`CELL`]-pixel square, conservative in the safe direction (see [`CELL`]), built from
/// a difference array and two prefix sums: `O(rects + cells)` to build and `O(1)` per query. That is
/// what makes a *search* affordable at all — the placement below probes several candidate columns
/// for every cluster, every frame the key is held, against two hundred rects rather than the twenty
/// the old `taken` list held.
///
/// It holds only the **static** ink of the frame. Boxes this pass has already placed stay in a small
/// `taken` list beside it, because they arrive one at a time and rebuilding an integral image per
/// box would trade the whole saving back.
struct FreeGround {
    origin: Vec2,
    cols: usize,
    rows: usize,
    /// Summed-area table of occupancy, `(cols + 1) * (rows + 1)`, row-major.
    sum: Vec<u32>,
}

impl FreeGround {
    fn build(window: Rect, ink: &[Rect]) -> Self {
        let size = window.size().max(Vec2::ONE);
        let cols = ((size.x / CELL).ceil() as usize).max(1);
        let rows = ((size.y / CELL).ceil() as usize).max(1);
        let origin = window.min;
        let (w, h) = (cols + 1, rows + 1);
        // A difference array: four writes per rect, then one 2-D prefix sum turns it into a count.
        let mut diff = vec![0i32; w * h];
        for r in ink {
            let c0 = (((r.min.x - origin.x) / CELL).floor() as i64).clamp(0, cols as i64) as usize;
            let r0 = (((r.min.y - origin.y) / CELL).floor() as i64).clamp(0, rows as i64) as usize;
            let c1 = (((r.max.x - origin.x) / CELL).ceil() as i64).clamp(0, cols as i64) as usize;
            let r1 = (((r.max.y - origin.y) / CELL).ceil() as i64).clamp(0, rows as i64) as usize;
            if c1 <= c0 || r1 <= r0 {
                continue;
            }
            diff[r0 * w + c0] += 1;
            diff[r0 * w + c1] -= 1;
            diff[r1 * w + c0] -= 1;
            diff[r1 * w + c1] += 1;
        }
        for y in 0..h {
            for x in 0..w {
                let mut v = diff[y * w + x];
                if x > 0 {
                    v += diff[y * w + x - 1];
                }
                if y > 0 {
                    v += diff[(y - 1) * w + x];
                }
                if x > 0 && y > 0 {
                    v -= diff[(y - 1) * w + x - 1];
                }
                diff[y * w + x] = v;
            }
        }
        // And a second prefix sum, this time over "is this cell occupied at all", so a rectangle's
        // occupancy is four lookups whatever its size.
        let mut sum = vec![0u32; w * h];
        for y in 0..rows {
            for x in 0..cols {
                let here = u32::from(diff[y * w + x] > 0);
                sum[(y + 1) * w + x + 1] =
                    here + sum[y * w + x + 1] + sum[(y + 1) * w + x] - sum[y * w + x];
            }
        }
        Self { origin, cols, rows, sum }
    }

    /// Is every cell this rectangle touches clear? A rectangle reaching outside the window is not
    /// free — there is no ground out there to stand on.
    fn is_free(&self, r: Rect) -> bool {
        let w = self.cols + 1;
        let c0 = ((r.min.x - self.origin.x) / CELL).floor();
        let r0 = ((r.min.y - self.origin.y) / CELL).floor();
        let c1 = ((r.max.x - self.origin.x) / CELL).ceil();
        let r1 = ((r.max.y - self.origin.y) / CELL).ceil();
        if c0 < 0.0 || r0 < 0.0 || c1 > self.cols as f32 || r1 > self.rows as f32 {
            return false;
        }
        // **A rect starting on the far cell boundary has no cell to stand on.** A zero-width probe
        // at `window.max` floors `c0` to `cols`, clears the guard above — `c1 > cols` is false when
        // `c1 == cols` — and the `max(c0 + 1)` below then indexes `sum` a column past its last,
        // which on the bottom row is a column past the table. There is no ground at the edge of the
        // grid, so the honest answer is the one a rect outside it already gets. Reachable from the
        // `deep` probe below, which is why this is a panic and not merely a wrong answer.
        if c0 as usize >= self.cols || r0 as usize >= self.rows {
            return false;
        }
        let (c0, r0) = (c0 as usize, r0 as usize);
        let (c1, r1) = ((c1 as usize).max(c0 + 1), (r1 as usize).max(r0 + 1));
        let taken = self.sum[r1 * w + c1] + self.sum[r0 * w + c0]
            - self.sum[r0 * w + c1]
            - self.sum[r1 * w + c0];
        taken == 0
    }
}

/// **[`ink`] run against a `World`**, so a test measures the editor's own answer rather than a
/// second copy of the rule.
///
/// `no_badge_cluster_draws_through_another` is the packer's contract, and a test that restated what
/// counts as ink would pass while the editor used a different definition — which is the exact drift
/// [`resolve`] is `pub` for. A `SystemState` is the one way to hand a plain `&mut World` the same
/// queries the system takes.
pub fn ink_now(world: &mut World) -> Vec<Rect> {
    let mut state: bevy::ecs::system::SystemState<(
        Query<Entity, With<BadgeLayer>>,
        Query<(
            Entity,
            &ComputedNode,
            &UiGlobalTransform,
            &InheritedVisibility,
            Option<&BackgroundColor>,
            Option<&BorderColor>,
            Option<&Text>,
            Option<&ImageNode>,
            Option<&chrome::Ground>,
        )>,
        Query<&ChildOf>,
    )> = bevy::ecs::system::SystemState::new(world);
    // `SystemState::get` is fallible in 0.19; a world that cannot satisfy the queries has no
    // interface to measure, which is the same answer as an empty one.
    let Ok((layers, painters, parents)) = state.get(world) else {
        return Vec::new();
    };
    // No layer means the overlay is not up, and nothing it could be measured against is either.
    let Some(layer) = layers.iter().next() else {
        return Vec::new();
    };
    ink(layer, &painters, &parents)
}

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
                // CHROME-OK: an absolute layer's origin, not a spacing step.
                left: Val::Px(0.0),
                // CHROME-OK: as above.
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
            GlobalZIndex(chrome::BADGE_Z),
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
/// verdict. [`anchor`] carries it, and [`place_badges`] reads it two ways: a row still partly in
/// view keeps its box on the rail with the leader's anchor end clamped into the fold — pointed at
/// from where scrolling would bring it back — and a row scrolled *wholly* out of sight goes quiet,
/// its cluster hidden until the row returns. Pinning the hidden ones at the pane edge was tried and
/// stacked four cell rows on top of each other there; quiet is the honest answer.
fn laid_out_rect(node: &ComputedNode, tf: &UiGlobalTransform) -> Option<Rect> {
    let size = node.size();
    (size != Vec2::ZERO).then(|| Rect::from_center_size(tf.translation, size))
}

/// **The one laid-out node claiming a [`keys::ControlId`], or none** — the single answer to *"is
/// this control on screen"*, asked by the census in [`rebuild_badges`] and by [`anchor`] alike.
///
/// Two visible nodes claiming one id is a bug rather than a tie to break, and choosing between them
/// would hide it. Answering `None` to **both** callers is what makes the bug survivable: [`resolve`]
/// sends a home nothing answers for to [`Home::Legend`], where the chord is drawn beside its own
/// prose.
///
/// # Two predicates is what made a duplicated id draw nothing at all
///
/// The census used to ask `.any(..)` — at-least-one — while this asked for exactly one. So a
/// duplicated id carried [`Home::Control`] through [`resolve`] and then found no anchor to be placed
/// against: the cluster stayed `Visibility::Hidden` and the verb was drawn neither on a control nor
/// in the legend, which is the one outcome [`Home`] exists to rule out. The exactly-one answer wins
/// because it is the answer that actually decides whether a box is drawn.
///
/// `every_control_the_census_homes_a_verb_at_is_on_screen` is the guard; the warning is the report
/// when the duplicate has already shipped, and it is diagnostics only — nothing about where the
/// badge goes depends on it.
fn sole_control(
    id: keys::ControlId,
    controls: &Query<(Entity, &chrome::Control, &ComputedNode, &UiGlobalTransform)>,
) -> Option<(Entity, Rect)> {
    let mut found = controls
        .iter()
        .filter(|(_, c, ..)| c.0 == id)
        .filter_map(|(e, _, node, tf)| laid_out_rect(node, tf).map(|r| (e, r)));
    let first = found.next()?;
    if found.next().is_some() {
        warn_once!("two visible controls claim {id:?}; its badges go to the legend");
        return None;
    }
    Some(first)
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
            return laid_out_rect(node, tf);
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
    // **The leader segments are swept with the clusters they serve.** They are children of the
    // layer, not of any cluster — spawned first so lines paint under boxes — so despawning the
    // clusters alone orphaned three hairlines per control home on every rebuild: an unbounded leak,
    // and the orphans kept their last rects and visibility, drawing ghost elbows over the next
    // overlay the moment the layer showed again.
    segs: Query<Entity, With<LeadSeg>>,
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
    // query's arbitrary order cannot make it rebuild for nothing — and only asked while the key is
    // down: with the overlay hidden this census was still a full scan of every control, every
    // frame, discarded unread. See [`laid_out_rect`] for what "on screen" has to mean.
    let key = want.then(|| {
        let on_screen: Vec<keys::ControlId> = keys::ControlId::ALL
            .into_iter()
            .filter(|id| sole_control(*id, &controls).is_some())
            .collect();
        (*mode, live.1, on_screen)
    });
    // **`showing` is believed only while its clusters exist.** `screen::close_the_door` sweeps the
    // layer's entities but resets no resources — `screen::OWNERSHIP` classifies this one `Door` and
    // says itself that the list changes no behaviour — so after leaving a kit mid-hold and opening
    // another, the stale key would match and suppress the first rebuild: a veil with zero badges.
    // An unmatched key rebuilds as before; a matched key with nothing spawned rebuilds too.
    if showing.0 == key && (key.is_none() || !clusters.is_empty()) {
        return;
    }
    showing.0 = key.clone();
    for entity in clusters.iter().chain(segs.iter()) {
        commands.entity(entity).despawn();
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

    // **The chord column a box's rows share: the longest chord in *that box*.**
    //
    // Measured exactly rather than estimated — FiraMono is the only face in the build, so
    // `chars * BODY_CHAR_W` is the width, not a guess at it.
    //
    // It used to read only the legend's badges, on the note that "computing it per home was the same
    // answer per home" — which was true precisely while the legend was the only home drawn as a box.
    // Now that every labelled cluster is one, a control's rows would be indented to the legend's
    // longest chord: `Space` pushed out to the width of `Cmd+Delete`, in a box that holds neither.
    let chord_col = |home: Home| -> f32 {
        live_badges
            .iter()
            .filter(|b| b.home == home)
            .map(|b| b.chord.chars().count())
            .max()
            .unwrap_or(0) as f32
            * BODY_CHAR_W
            + 1.0
    };

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
                        // **All one width**, so the box reads as a block rather than as a ragged
                        // stack — `Stretch` takes the widest row and gives it to every row. A
                        // cluster of bare chords wants the opposite: each chip hugs its own chord,
                        // because there they are labels on things and not a table.
                        align_items: if labelled {
                            AlignItems::Stretch
                        } else {
                            AlignItems::Start
                        },
                        max_width: if labelled { Val::Auto } else { Val::Px(CLUSTER_W) },
                        // **A labelled cluster is ONE box, and its rows are rows.**
                        //
                        // This was true of the legend alone, and the asymmetry was a bug with two
                        // faces. A control that owns four chords drew **four bordered boxes** — and
                        // a cluster has exactly **one** leader, so three of the four had no line to
                        // anything. Reported from the keyboard: *"if we have a keyboard legend that
                        // relates to a section of UI components, then the legend should have a
                        // single box instead of being split between two. So every box should have
                        // one line to the component it belongs in."*
                        //
                        // The other face is where the line lands. `place_badges` aims the leader at
                        // the **cluster's** centre, and with the cluster invisible that centre falls
                        // in the *gap between two chips* — a line ending in empty space next to the
                        // boxes it belongs to. Also reported: *"the lines are still not connecting
                        // to the keyboard legend box so that it falls in the middle."* One box, one
                        // centre, one line: the same edit answers both.
                        //
                        // A band's cluster still carries no box — there each badge is its own chip
                        // beside a control whose own words are already the verb.
                        padding: if labelled { chrome::CHIP_PAD } else { UiRect::ZERO },
                        border: if labelled {
                            UiRect::all(Val::Px(chrome::EDGE_W))
                        } else {
                            UiRect::ZERO
                        },
                        column_gap: Val::Px(GAP_TIGHT),
                        row_gap: Val::Px(GAP_TIGHT),
                        ..default()
                    },
                    BackgroundColor(if labelled { PANEL_BG } else { Color::NONE }),
                    BorderColor::all(if labelled { KEY } else { Color::NONE }),
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
                        one_badge(
                            c,
                            badge,
                            labelled,
                            chord_col(*home),
                            if legend { DOES_COL } else { CONTROL_DOES_COL },
                        );
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
    match home {
        Home::Control(id) => {
            let (entity, at) = sole_control(id, controls)?;
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
            laid_out_rect(node, tf).map(|at| Anchored { at, fold: None, banded: false })
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

/// **The elbow between an anchor point and a box**, as up to three hairline rects: the **reach** out
/// of the anchor, the **run** down the corridor lane, and the **landing** into the box. A box level
/// with its anchor draws the reach alone, which is the common case and reads as plain adjacency.
///
/// A free function rather than a closure because [`lanes_for`] has to build a candidate leader for
/// every lane it considers, and a leader drawn one way and tested another is two answers to the
/// question this whole module exists to answer once.
fn elbow_of(a: Vec2, lane_x: f32, edge: f32, mid: f32, thick: f32) -> [Option<Rect>; 3] {
    let h = |x0: f32, x1: f32, y: f32| {
        Rect::from_corners(
            Vec2::new(x0.min(x1), y - thick * 0.5),
            Vec2::new(x0.max(x1), y + thick * 0.5),
        )
    };
    if (a.y - mid).abs() <= thick {
        // Level with its anchor: one straight line, which is what it looks like anyway.
        return [Some(h(a.x, edge, a.y)), None, None];
    }
    let v = Rect::from_corners(
        Vec2::new(lane_x - thick * 0.5, a.y.min(mid)),
        Vec2::new(lane_x + thick * 0.5, a.y.max(mid)),
    );
    [
        Some(h(a.x, lane_x, a.y)),
        Some(v),
        Some(h(lane_x, edge, mid)),
    ]
}

/// **Do two leaders touch anywhere?** Any shared ground at all — a perpendicular crossing, or two
/// runs lying along each other, which is the same failure seen from the side.
fn leaders_meet(a: &[Option<Rect>; 3], b: &[Option<Rect>; 3]) -> bool {
    a.iter().flatten().any(|p| {
        b.iter().flatten().any(|q| {
            let i = p.intersect(*q);
            i.width() > 0.0 && i.height() > 0.0
        })
    })
}

/// **Where a leader leaves its anchor and where it lands** — everything a lane choice needs, and
/// nothing a lane choice may change.
#[derive(Clone, Copy)]
struct Tie {
    /// As deep inside the control as a hairline can reach without crossing anything readable, at
    /// the height it leaves from.
    at: Vec2,
    /// The box edge the landing meets, and the height it arrives at.
    edge: f32,
    mid: f32,
    /// Lane 0's `x` for this side's band, and the step from one lane to the next.
    lane0: f32,
    step: f32,
}

impl Tie {
    fn lane_x(&self, k: usize) -> f32 {
        self.lane0 + self.step * (k as f32 + 0.5)
    }

    fn leader(&self, k: usize, thick: f32) -> [Option<Rect>; 3] {
        elbow_of(self.at, self.lane_x(k), self.edge, self.mid, thick)
    }
}

/// **A lane for every leader on one side, chosen so that no two of them cross.**
///
/// Two right-angle leaders sharing one corridor cannot cross *provided* their boxes keep their
/// anchors' order — that is the whole reason [`place_badges`] sorts a rail by anchor `y`. What one
/// corridor cannot do is let a reader tell them apart, so every leader gets a lane
/// ([`LANE`]) — and lanes reintroduce the crossing the single corridor had ruled out, because a
/// reach now has to pass every lane inside its own.
///
/// **The order lanes are tried in** is the one that makes the assignment safe analytically: the
/// leader that leaves highest takes the outer lane. Both ways a pair can conflict — one's reach
/// passing the other's run, or one's landing doing it — resolve the same direction *provided* the
/// boxes keep their anchors' order, and the packing ladder is allowed to break that order when it is
/// the only way to find a box any ground at all. So the preference is where the search **starts**,
/// not where it ends.
///
/// **It is a search and not a walk**, because a greedy pass can only ever move the leader it is
/// currently placing, and a conflict is as often a demand to move one already placed. This
/// backtracks instead, in preference order, so the shape stays the analytic one wherever the
/// analytic one works — which, since `place_badges` packs a whole side against one floor, is now
/// everywhere the boxes are laid out in their anchors' order.
///
/// **What the search is left for** is the two shapes that one floor does not order: a banded box,
/// whose leader is placed before any of this and never enters `ties`, and the residue of `compact`
/// mode, where a box packed from the top of the stage can sit above its own anchor and depart from
/// the anchor's *top* edge rather than its bottom. Both are rare and both are local.
///
/// Bounded twice over: by the lane count per position, and by a node budget — a rail of eight with
/// ten lanes has more assignments than is worth exploring, and an overlay that thought about its
/// leaders for a frame would be a worse failure than a crossing.
///
/// **When the walk does not finish, the analytic preference stands**, and that is the same answer
/// the walk starts from rather than a degraded second one: with the boxes in their anchors' order
/// it is crossing-free by Bekos et al.'s argument, which is the case this is reached in.
/// `no_two_leaders_cross` is what says so on a real layout.
fn lanes_for(ties: &[Tie], lanes: usize, thick: f32) -> Vec<usize> {
    let n = ties.len();
    if n == 0 {
        return Vec::new();
    }
    // **Ranked by where each leader actually leaves its anchor, not by where its anchor is.**
    //
    // The rail is packed in the order of the anchors' *centres*, and a leader departs from the point
    // on its anchor nearest its box — which for a tall control is nowhere near the centre. On the
    // Map that inverted two of three: the detail pane's leader left at y 240, below the UNDER row's
    // anchor at 214, so the pane took the outer lane and its reach crossed the row's run at exactly
    // the height the ranking said it could not.
    let mut rank: Vec<usize> = (0..n).collect();
    rank.sort_by(|x, y| {
        ties[*x]
            .at
            .y
            .total_cmp(&ties[*y].at.y)
            // `Query` order is not stable across `App` instances, so a tie needs a stated key or
            // the overlay lays itself out differently on two runs of the same test.
            .then(x.cmp(y))
    });
    // The lanes each position tries, in order: its preference first, then outward from it.
    let orders: Vec<Vec<usize>> = (0..n)
        .map(|high| {
            let pref = lanes.saturating_sub(1 + high);
            let mut order: Vec<usize> = (0..lanes).collect();
            order.sort_by_key(|k| (k.abs_diff(pref), *k));
            order
        })
        .collect();

    let mut chosen = vec![0usize; n];
    let mut drawn: Vec<[Option<Rect>; 3]> = Vec::with_capacity(n);
    let mut budget = 20_000usize;

    // Depth-first over positions, in rank order. Depth is `n`, so the recursion is as deep as a
    // rail is long.
    fn walk(
        at: usize,
        ties: &[Tie],
        rank: &[usize],
        orders: &[Vec<usize>],
        thick: f32,
        chosen: &mut Vec<usize>,
        drawn: &mut Vec<[Option<Rect>; 3]>,
        budget: &mut usize,
    ) -> bool {
        if at == rank.len() {
            return true;
        }
        let p = rank[at];
        for &k in &orders[at] {
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            let cand = ties[p].leader(k, thick);
            if drawn.iter().any(|d| leaders_meet(&cand, d)) {
                continue;
            }
            chosen[p] = k;
            drawn.push(cand);
            if walk(at + 1, ties, rank, orders, thick, chosen, drawn, budget) {
                return true;
            }
            drawn.pop();
        }
        false
    }

    if walk(0, ties, &rank, &orders, thick, &mut chosen, &mut drawn, &mut budget) {
        return chosen;
    }
    // No clean assignment inside the budget: the preference, which is the shape the analytic
    // argument gives and which the walk began from.
    (0..n)
        .map(|p| {
            let high = rank.iter().position(|r| *r == p).unwrap_or(0);
            lanes.saturating_sub(1 + high)
        })
        .collect()
}

/// **The first `y` at or below `pref` for a box of `size` at `x`, clear of every box already
/// placed** — the last rung of the ladder, and the one that cannot fail.
///
/// Bounded by `taken.len()`: each step lands this box just past one rect it hit and `y` only grows,
/// so the loop cannot run longer than the list it is dodging — and after stepping past a rect, that
/// rect can never hit again. There is **no give-up arm**. The old placement stopped when its bound
/// ran out and *let the overlap show*, and a real kit on a real window priced that honesty: the
/// legend under the piece list's boxes, four cell rows buried beneath their own neighbours. A box
/// that cannot hug its row now sits further down the same rail, and the leader carries the
/// attachment — that trade is what the leader exists to buy.
///
/// It knows nothing of [`FreeGround`], deliberately: this is the answer the overlay shipped with,
/// kept as the floor of the ladder so a screen with no free ground left degrades to what worked
/// rather than to a badge that is not drawn.
fn settle_past(pref: f32, size: Vec2, x: f32, taken: &[Rect], gap: f32) -> f32 {
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

/// **The first `y` at or below `pref` where a box of `size` at `x` stands on ground nothing uses**,
/// or `None` if this column has none left above `bottom`.
///
/// Three obstacle sets, because they arrive differently: [`FreeGround`] holds the frame's static ink
/// and answers in constant time; `taken` holds the boxes this pass has already placed, which are few
/// and change with every placement; and `world` is the one rectangle the world is drawing, passed
/// beside the grid rather than baked into it so that the ladder can ask the same column twice —
/// once dodging the map, once not — for the price of one rect test rather than a second grid. Stepping past a `taken` rect skips a whole box at once;
/// stepping past ink advances one [`CELL`], which is what makes the walk down a dock past a list of
/// rows finite rather than clever.
///
/// The `guard` is a stated bound rather than a `while true`: the column is finite and `y` only
/// grows, so the count of steps that can happen is arithmetic, and this crate does not ship loops
/// whose termination is an argument.
fn settle_free(
    pref: f32,
    size: Vec2,
    x: f32,
    ground: &FreeGround,
    taken: &[Rect],
    gap: f32,
    bottom: f32,
) -> Option<f32> {
    let mut y = pref;
    let cap = ((bottom - pref).max(0.0) / CELL) as usize + taken.len() + 2;
    for _ in 0..cap {
        if y + size.y > bottom {
            return None;
        }
        let me = Rect::from_corners(Vec2::new(x, y), Vec2::new(x, y) + size);
        match taken.iter().find(|t| covers(**t, me)) {
            Some(hit) => y = hit.max.y + gap,
            None if ground.is_free(me) => return Some(y),
            None => y += CELL,
        }
    }
    None
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
fn settle_legend(
    size: Vec2,
    stage: Rect,
    ground: &FreeGround,
    taken: &[Rect],
    gap: f32,
    inset: f32,
) -> Vec2 {
    // **The corner is inset by the frame's own margin, not by a multiple of the gap.** It was
    // `gap * 3`, which arrived at exactly [`chrome::MARGIN`] while [`REACH`] was 4 — an arithmetic
    // coincidence, and widening the reach would have walked the legend a further 36 px in from the
    // corner it exists to be learnable at. The distance from a window edge and the distance from a
    // badge are different questions; this is the first one.
    let corner = (stage.max - size - Vec2::splat(inset)).max(stage.min);
    // **Three sources of candidate column, in one list sorted nearest-the-corner first.** The
    // corner's own; one flush against the left edge of each box already placed, which is the
    // tightest packing available; and a coarse sweep leftward, which is what finds a gap that no
    // box happens to have an edge at — the case that arrived with the ink census, since most of
    // what the legend is now dodging is a panel rather than a badge.
    let sweep = ((stage.width() / (CELL * 4.0)).ceil() as usize).min(256);
    let mut columns: Vec<f32> = std::iter::once(corner.x)
        .chain(taken.iter().map(|t| t.min.x - size.x - gap))
        .chain((1..=sweep).map(|k| corner.x - k as f32 * CELL * 4.0))
        .filter(|x| *x >= stage.min.x && *x <= corner.x)
        .collect();
    columns.sort_by(|a, b| b.total_cmp(a));
    columns.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    // Bounded: at most `taken + 1` climbs past a box, plus one cell-step per cell of the column.
    let cap = ((corner.y - stage.min.y).max(0.0) / CELL) as usize + taken.len() + 2;
    // **One walk, not two.** It used to run the whole search dodging the world's box and then again
    // without it, because a legend that stands somewhere is the one thing this block is for. The
    // world is faded while the key is held now (`chrome::WORLD_HELD`), so there is nothing here to
    // dodge and nothing to fall back from — see [`place_badges`] on why routing around the envelope
    // cost more ground than it saved.
    for x in columns.iter().copied() {
        let mut y = corner.y;
        for _ in 0..cap {
            if y < stage.min.y {
                break;
            }
            let me = Rect::from_corners(Vec2::new(x, y), Vec2::new(x, y) + size);
            match taken
                .iter()
                .filter(|t| covers(**t, me))
                .map(|t| t.min.y)
                .reduce(f32::min)
            {
                // Climb clean past the whole box, which is the tightest honest answer.
                Some(top) => y = top - size.y - gap,
                None if ground.is_free(me) => return Vec2::new(x, y),
                // Ink has no box to climb past — step one cell and ask again.
                None => y -= CELL,
            }
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
    painters: Query<(
        Entity,
        &ComputedNode,
        &UiGlobalTransform,
        &InheritedVisibility,
        Option<&BackgroundColor>,
        Option<&BorderColor>,
        Option<&Text>,
        Option<&ImageNode>,
        Option<&chrome::Ground>,
    )>,
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
        .and_then(|(node, tf)| laid_out_rect(node, tf))
    else {
        return;
    };

    // `Val::Px` is multiplied by `UiScale` and everything above is in surface pixels, so this is the
    // one conversion — `compose::place_labels` paid for putting it anywhere else, and every label it
    // drew landed 20% further from the corner than the point it named. A zero or negative scale is a
    // host misconfiguration rather than a state to render around; guarded so it cannot make a NaN.
    let scale = if ui_scale.0 > 0.0 { ui_scale.0 } else { 1.0 };
    let reach = REACH * scale;
    let stack = STACK * scale;
    let lane = LANE * scale;
    let thick = (LEAD_THICK * scale).max(1.0);

    // **The stage: the hole the world is drawn through**, intersected with the window because the
    // viewport is a flex item and on a window too narrow for both docks it overflows the frame.
    // Boxes and the legend live here and nowhere else — a box over a panel would cover the words
    // that identify what some other badge points at, and the docks are not veiled ground.
    let stage = rects
        .get(frame.viewport)
        .ok()
        .and_then(|(node, tf)| laid_out_rect(node, tf))
        .map(|r| r.intersect(window))
        .unwrap_or(window);

    let mut items: Vec<_> = clusters.iter_mut().collect();
    // **Nothing on screen, nothing to measure.** With the key up `rebuild_badges` has despawned
    // every cluster, and the census below is a walk of the whole interface — which, run every frame
    // for an empty loop, is exactly the cost the rebuild's own on-screen scan was just moved out of.
    if items.is_empty() {
        return;
    }
    items.sort_by_key(|(cluster, ..)| place_order(cluster.0));
    let anchors: Vec<Option<Anchored>> = items
        .iter()
        .map(|(cluster, node, ..)| {
            (node.size() != Vec2::ZERO)
                .then(|| anchor(cluster.0, &frame, &rects, &controls, &parents, &folds))
                .flatten()
        })
        .collect();

    // **One ground and one rectangle, and the difference between them is the whole tier system.**
    //
    // The grid is ground **no reader needs** — the interface as laid out, minus the containers whose
    // fill is only ground ([`chrome::Ground`]). Nothing may ever stand on it: a chord over a row is
    // a row you cannot read, and that is the failure `no_badge_cluster_draws_through_another` exists
    // to catch.
    //
    // **Nothing dodges the world any more, and the fade is why.**
    //
    // A badge used to detour around the box the world draws, on the argument that a chord over the
    // map is a HUD. The detour is what ran the Tiles tab out of room: the tile's envelope is a
    // 1 x 4 x 1 m box that projects to a tall rectangle straight down the middle of the stage, the
    // filter's badge detoured 530 px below its own row to clear it, that dragged the side's floor
    // down with it, and the piece list's badge then had nowhere left to stand but on top of its
    // neighbour. One detour, three displaced boxes, and the overlap the author actually saw.
    //
    // `chrome::WORLD_HELD` answers it at the source instead: while the key is held the envelope
    // drops to a quarter alpha and reads as background, so a badge — opaque, bordered, and drawn
    // over it — is legible standing right on it. The author's own framing: *"we should use fading
    // of certain UI elements to ensure what we are visually communicating to users instead."*
    // Routing around a thing and quietening it are two answers to one question, and this keeps the
    // one that costs no ground.
    //
    // It stays **beside** the grid rather than inside a second one: the two censuses differ by
    // exactly this rectangle, so building a whole second summed-area table to express that would
    // pay 56,000 cells a frame for one rect test.
    let ground = FreeGround::build(window, &ink(layer, &painters, &parents));

    // **The rails: every un-banded control box column-aligns just past the widest anchor on its
    // side.** One straight rail per side reads as one thing, and it is what gives every leader's
    // vertical run a corridor — the strip between dock edge and rail — where no box can stand.
    let mid = window.center().x;
    let side_of = |a: &Anchored| a.at.center().x <= mid;
    // **The dock a side's boxes may stand inside**, which is the ground this whole change is about:
    // the column below a short panel, and the slack a pane leaves between its last row and its
    // hint. A dock has no fill of its own (`chrome::spawn_frame`) and a panel's fill is
    // `chrome::Ground`, so all of that is ink-free — it simply had nowhere to be asked about
    // before, because a box could only stand in the stage.
    let dock_of = |left: bool| -> Option<Rect> {
        rects
            .get(if left { frame.left } else { frame.right })
            .ok()
            .and_then(|(node, tf)| laid_out_rect(node, tf))
    };
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
    let elbow = |a: Vec2, lane_x: f32, edge: f32, mid: f32| elbow_of(a, lane_x, edge, mid, thick);

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
        // **Clamped into the window, on both axes.** Every other placement is bounded by
        // `FreeGround::is_free` refusing a rect that reaches outside it; a banded box takes neither
        // that ladder nor the dock's bound, so the bound has to be stated here or a chip belonging
        // to the chrome bar's rightmost control is drawn off the edge of the screen — and `y` was
        // never bounded at all. `.max(window.min.x)` keeps `clamp`'s range the right way round for a
        // box wider than the window it is being put in; an inverted range panics.
        let x = if leading >= window.min.x {
            leading
        } else {
            a.at.max.x + reach
        }
        .clamp(window.min.x, (window.max.x - size.x).max(window.min.x));
        let y = (a.at.center().y - size.y * 0.5)
            .clamp(window.min.y, (window.max.y - size.y).max(window.min.y));
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
        // **Ordered by the edge a leader will leave from, not by the anchor's centre.**
        //
        // Order preservation is the whole crossing-free argument, and it has to be the order of the
        // thing that actually varies: a leader departs from the point on its anchor nearest its box,
        // and `settle_free` only ever puts a box at or below the anchor's top, so that point is the
        // anchor's **bottom** edge. For a row the two keys agree to within half a line. For a tall
        // pane they do not, and on the Map they disagreed by fifty pixels — the detail pane (bottom
        // 242) sorted *above* the UNDER row (bottom 222) on centres, so its box was packed higher
        // than the row's while its leader left lower. That put the pane's whole run inside the row's,
        // and two nested runs crossing one strip must cross: `lanes_for` searched every assignment
        // and there was none.
        rail.sort_by(|x, y| {
            let (ax, ay) = (
                anchors[*x].as_ref().map(|a| a.at.max.y).unwrap_or(0.0),
                anchors[*y].as_ref().map(|a| a.at.max.y).unwrap_or(0.0),
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
            // **One floor for the whole side, across both of its columns.**
            //
            // It was one floor per column, on the argument that a shared floor would have a box in
            // the dock push the rail's next box down for no reason. That is true, and it is the
            // wrong trade. Two floors let a dock box and a rail box invert against their anchors,
            // and that inversion is a crossing **no lane assignment can undo**: a rail leader's
            // reach crosses the whole gutter a dock leader runs down, at whatever height it leaves
            // at, so there is no lane either can take to miss the other. `lanes_for` searched every
            // assignment, found none, and returned its preference — the packer was choosing which
            // crossing to have.
            //
            // Bekos, Cornelsen, Fink, Hong, Kaufmann, Nollenburg, Rutter & Symvonis 2015,
            // *Many-to-One Boundary Labeling with Backbones* (`10.7155/jgaa.00379`), is why this is
            // a fix rather than a preference: minimising leader crossings is efficient **in the
            // case of fixed label order** and **NP-hard in the case of flexible label order**. Two
            // floors made the order flexible, which is how a bounded search over lanes ended up
            // standing in for a problem no bounded search can settle. One floor fixes the order,
            // and buys the sentence the crossing-free argument actually needs: one non-decreasing
            // `mid` per side.
            //
            // The cost is density, and it is the cost that was chosen: a badge may sit further down
            // its column, away from the row it names, with the leader carrying the association
            // across that distance. That is what the leader was bought for.
            let side = rail_of(left);
            // **A side with no leaders reserves no corridor.** `left_edge`/`right_edge` are still
            // their `f32::MIN`/`f32::MAX` sentinels when nothing un-banded anchors on this side, so
            // the band built from them below is not two empty lanes — it is a rect pinned at the far
            // end of the number line, pushed into `taken`, telling the legend and the other side's
            // boxes that ground they can see is spoken for. Everything after this line either
            // iterates `side` or reads a band only `side` uses, and `lanes_for` returns nothing for
            // zero leaders, so an empty side has nothing to do here.
            if side.is_empty() {
                continue;
            }
            let mut floor = stage.min.y;
            // **The corridor is a band now, one [`LANE`] per leader on this side.** Reserved whole
            // and up front, so a box can never stand where a lane might later run — which is what
            // lets the lanes be chosen *after* every box is placed, from the geometry they all end
            // up with, rather than guessed at one at a time.
            // **Two lanes more than there are leaders.** One each is enough for the preference to
            // be honoured; the spares are what the greedy walk spends when it cannot be — a box in
            // the dock hooks back across every lane inside its own to reach its own edge, and that
            // is the one shape the analytic order does not rule out.
            let lanes = side.len() + 2;
            let width = lanes as f32 * lane;
            let (band_near, band_far) = if left {
                (left_edge + reach, left_edge + reach + width)
            } else {
                (right_edge - reach - width, right_edge - reach)
            };
            taken.push(Rect::from_corners(
                Vec2::new(band_near, stage.min.y),
                Vec2::new(band_far, stage.max.y),
            ));
            let band_mid = (band_near + band_far) * 0.5;

            // `a_x` and `at_home` ride along: the mixed-side rule below needs both, and it
            // cannot know either until every box on the side has been placed.
            let mut ties: Vec<(usize, Tie, f32, bool)> = Vec::with_capacity(side.len());
            for i in side {
                // `rail_of` filters on `is_some`, so this is never `None` today — but stating that
                // with a panic would hand a future edit to `rail_of` a crash instead of a skipped
                // box, and this crate refuses panic paths (`CLAUDE.md`).
                let Some(a) = anchors[i].as_ref() else { continue };
                if let Some(f) = a.fold {
                    let seen = a.at.intersect(f);
                    if seen.width() <= 0.0 || seen.height() <= 0.0 {
                        continue;
                    }
                }
                let size = items[i].1.size();
                let (rail_x, a_x) = if left {
                    (band_far + reach, a.at.max.x)
                } else {
                    (band_near - reach - size.x, a.at.min.x)
                };
                // The vertical bound is the stage's, on every rung: the stage and the docks are
                // both children of the frame's body, so this is the band between the door strip and
                // the status band — and a box over a band would cover the chrome that names where
                // you are.
                let (top, bottom) = (stage.min.y, stage.max.y);
                let settled = |floor: f32| {
                    if compact {
                        floor
                    } else {
                        a.at.min.y.clamp(top, (bottom - size.y).max(top)).max(floor)
                    }
                };

                // **A ladder, nearest the anchor first.** Boundary labeling (Bekos et al.) says
                // where a box may go once it has left its anchor; it does not say what ground it may
                // stand on, and until this ladder existed the answer was "the rail, and the rail
                // only" — which put a box on the map while the dock beside it had four hundred
                // unused pixels.
                //
                // Two axes, climbed outermost-first, so the cheapest concession is always spent
                // before the dearer one:
                //
                // 1. **the ground** — dodging the world's own box before standing on it, because
                //    a chord over the map is a HUD and a chord over a row is a row you cannot read;
                // 2. **the column** — the anchor's own dock before the rail, because a box beside
                //    the rows it names needs no reading at all.
                //
                // **There used to be a third, and it is what put the crossings on screen.** Between
                // those two sat *the start*: this column's floor first, then the top of the stage.
                // The second arm let a box that could not fit below the floor jump above it and
                // stand on the free ground up there — which is precisely an order inversion, and
                // the comment admitted as much ("which does not, and is precisely what the leader
                // was bought for"). A leader can carry a box a long way from its anchor; what it
                // cannot do is carry it *past another box* without the two lines crossing. So every
                // rung now starts at the side's floor, and a box that will not fit below it goes
                // further down rather than back up.
                //
                // **The dock column** right-aligns the box with its anchor, so it reads as part of
                // the same column, and hangs its leader in the same corridor the rail uses — the
                // strip between the widest anchor and the rail, where no box on either column may
                // stand.
                //
                // **A row inside a scrolling pane may use it too, and that is new.** The rule was
                // that a fold's rows come and go, so the ground beside them belongs to whatever
                // scrolls into it. True — and it cost more than it bought. Every control on the
                // Meshes and Tiles tabs lives in the detail pane's fold, so *no* box on those tabs
                // could ever stand in its own dock: all of them went to the rail, both rails landed
                // in a stage about 610 logical px wide, and the boxes needed more than that. The
                // last-resort placement then clamped one on top of another. Reported from the
                // keyboard: *"the tiles tab is super muddy"* — with about 655 x 430 px of the left
                // dock sitting empty below MEMBERS the whole time.
                //
                // What makes it safe is not that the objection was wrong but that it is now handled
                // elsewhere: the ground is still tested against the live [`FreeGround`], so a box
                // only ever stands where nothing is drawn *now*, and a row that scrolls beyond its
                // fold already goes quiet rather than being pointed at from nowhere.
                // **Inset by the width of its own lane band**, leaving a gutter between the box and
                // its anchor's edge for the leader to turn in. See the `lane0` match below for why
                // that gutter is what makes a dock leader layable at all.
                let home_x = if left {
                    a.at.max.x - width - size.x
                } else {
                    a.at.min.x + width
                };
                // **The dock has to hold the box *and* the gutter its leader turns in.** The gutter
                // sits on the anchor's side of the box — outboard on the right dock, inboard on the
                // left — so the span is sided. This read `home_x + size.x + width <= d.max.x +
                // width`, where `+ width` cancelled on both sides and the test measured the box
                // alone: a box whose gutter ran off the dock still earned the dock, and its leader
                // then turned outside it.
                let (span_lo, span_hi) = if left {
                    (home_x, home_x + size.x + width)
                } else {
                    (home_x - width, home_x + size.x)
                };
                let home_fits =
                    dock_of(left).is_some_and(|d| span_lo >= d.min.x && span_hi <= d.max.x);
                let mut chosen: Option<(f32, f32, bool)> = None;
                'ladder: for (cx, at_home) in [(home_x, true), (rail_x, false)] {
                    if at_home && !home_fits {
                        continue;
                    }
                    if let Some(y) =
                        settle_free(settled(floor), size, cx, &ground, &taken, stack, bottom)
                    {
                        // **The dock is earned by being level with the row, not merely by fitting
                        // in the panel.**
                        //
                        // The whole argument for standing in the anchor's own column is that *"a box
                        // beside the rows it names needs no reading at all"* — and a box that had to
                        // travel down the panel to find a gap is not beside anything. It is wedged
                        // between two other rows, which reads as a badge sitting **on** the
                        // interface; and because a dock leader starts at the anchor's outer edge
                        // rather than reaching in to the label, its line then runs *outward*, away
                        // from the row it names. Reported from the keyboard: *"Type Id and Mount
                        // look weird, they are over the UI and point to the workspace. It should be
                        // the other way around."*
                        //
                        // It should, and the rail is where that shape already exists: a rail box
                        // stands on the stage — ground nothing else uses — and its leader walks
                        // **inward** to land on the label itself. So a dock placement that is not
                        // level with its anchor yields to one, and the badge ends up over the
                        // workspace pointing into the panel, which is the reading order asked for.
                        let beside_its_row = y < a.at.max.y;
                        if !at_home || beside_its_row {
                            chosen = Some((cx, y, at_home));
                            break 'ladder;
                        }
                    }
                }
                // **The floor of the ladder: the rail, unconditionally** — the answer this overlay
                // shipped with. A screen with no free ground left at all degrades to what worked
                // rather than to a verb that is not drawn, and the overlap ratchet is what reports
                // that it happened.
                // **The floor of the ladder: the rail, unconditionally** — never the dock. A box
                // forced into a dock that had no free ground for it stands on the pane's own rows,
                // and a chord over a word is the one thing this overlay may not do; out on the rail
                // it stands over the stage instead. Measured, when this briefly read `column`:
                // `MESHES: Control(CellGrid) covers 137x13 px of ink at Vec2(28.0, 668.0)`.
                let (x, y, at_home) = chosen.unwrap_or_else(|| {
                    // **Clamped into the window before the column is settled, not after.** `rail_x`
                    // walks outward with the lane band — `(side.len() + 2) * lane` — so a side with
                    // six leaders puts the rail past the edge on a narrow window, and this arm takes
                    // neither the `is_free` ladder nor the dock's bound. Clamped first so
                    // `settle_past` stacks against the column the box will actually stand in; the
                    // high bound is `.max(lo)` because `f32::clamp` panics on an inverted range.
                    let x = rail_x.clamp(window.min.x, (window.max.x - size.x).max(window.min.x));
                    let y = settle_past(settled(floor), size, x, &taken, stack)
                        .clamp(top, (bottom - size.y).max(top));
                    (x, y, false)
                });
                let pos = Vec2::new(x, y);
                plans[i].pos = Some(pos);
                // **The box edge the leader lands on is the one facing the corridor**, which is the
                // left edge for a rail box (the corridor is inboard of it) and the right edge for a
                // box standing in the left dock (the corridor is outboard). One expression rather
                // than a branch per rung: the corridor is the fact, the side is a consequence.
                let box_edge = if (x - band_mid).abs() <= (x + size.x - band_mid).abs() {
                    x
                } else {
                    x + size.x
                };
                // **The leader's anchor end: the nearest point on the anchor's edge, not its
                // centre.** A list is taller than its badge, and centre-attachment drew a run the
                // full half-height of the palette — long enough that the legend, refusing to cover
                // it, gave up its own corner. Nearest-point is the standard callout attachment and
                // collapses the common case back to a bare stub. Clamped into the fold when the
                // row is beyond it — pointed at from where scrolling would bring it back.
                let a_y = {
                    // Both ranges keep `lo <= hi`: a control or a fold squashed shorter than two
                    // hairlines (a collapsed pane on a short window) would otherwise hand
                    // `f32::clamp` an inverted range, which panics.
                    let into = |r: Rect| {
                        let lo = r.min.y + thick;
                        (lo, (r.max.y - thick).max(lo))
                    };
                    let (lo, hi) = into(a.at);
                    let near = (y + size.y * 0.5).clamp(lo, hi);
                    match a.fold {
                        Some(f) => {
                            let (lo, hi) = into(f);
                            near.clamp(lo, hi)
                        }
                        None => near,
                    }
                };
                // **How far into the control the leader may reach.**
                //
                // It used to stop at `a_x` — the anchor's own outer edge — and for a row that spans
                // its pane that edge *is* the pane's inner edge, so every leader on a side arrived
                // at the same vertical line and stopped. Reported from the keyboard: *"the lines
                // actually go underneath the UI panels… can we have this go over and point directly
                // to the label of what action it's going to impact?"* They were never underneath —
                // the layer is `GlobalZIndex(500)` and the panels are 0 — they simply had nowhere
                // further to go.
                //
                // So the leader walks **inward** until the next step would cross something a reader
                // needs, and lands there: on a heading row that is the heading itself, on a row
                // carrying a value it is the value. The same census the boxes stand on answers it,
                // so a leader can no more strike through a word than a box can cover one.
                //
                // **If nothing stops it, it does not go in at all.** A walk that reaches the far
                // side of its anchor has found no label at this height — an empty stretch of pane —
                // and a line ending in the middle of nothing points at nothing.
                let deep = {
                    let inward = if left { -CELL } else { CELL };
                    let limit = if left { a.at.min.x } else { a.at.max.x };
                    let me = Rect::from_corners(pos, pos + size);
                    let mut d = a_x;
                    let mut stopped = false;
                    // Bounded by the anchor's own width: one step per cell across it, and `d` only
                    // moves one way.
                    for _ in 0..((a.at.width() / CELL) as usize + 2) {
                        let next = d + inward;
                        if (next - limit) * inward > 0.0 {
                            break;
                        }
                        let probe = Rect::from_corners(
                            Vec2::new(d.min(next), a_y - thick),
                            Vec2::new(d.max(next), a_y + thick),
                        );
                        let blocked = !ground.is_free(probe)
                            || covers(me, probe)
                            || taken.iter().any(|t| covers(*t, probe));
                        if blocked {
                            stopped = true;
                            break;
                        }
                        d = next;
                    }
                    if stopped { d } else { a_x }
                };

                // **The line meets the box at its centre**, not at whichever edge happens to be
                // nearest.
                //
                // Nearest-point was tried, on the symmetry with the anchor end (`f46db80`) and
                // because a shorter run gives another leader's reach less height to cross — it was
                // worth one real crossing on the Map. It reads wrong: a line arriving at a box's top
                // or bottom corner looks like it has clipped the box on its way past, and a badge is
                // a label with two lines of text in it, not a point. Reported from the keyboard:
                // *"make sure the line connects in the center of the text box as opposed to the
                // bottom."* The crossing it cost is `lanes_for`'s problem, which is what `lanes_for`
                // is for.
                let mid = y + size.y * 0.5;
                // **Where this leader leaves, and where its lanes run.**
                //
                // A **rail** box is across the stage from its anchor, so its leader has to be
                // followed — it leaves from `deep`, on the label itself, runs out to the band beside
                // the dock and lands outward on the box. Every leg goes the same way.
                //
                // A **dock** box stands in its anchor's own column. Laning it out in that same band
                // means the landing turns round and comes all the way back, and a leader that
                // doubles back **cannot be laned at all**: its reach says "put my lane outside
                // yours" to every leader whose run spans my departure, and its landing says "put my
                // lane inside yours" to every leader whose run spans my arrival — and on the Map's
                // three rows both demands land on the same pair. There is no assignment; the
                // packer just picks which crossing to have.
                //
                // So a dock box is **inset by the band's width** and lanes in the gutter that
                // leaves, between its own edge and its anchor's. Reach, run and landing then all go
                // the same way — a staircase in, not a hook — and the contradiction is gone. It
                // leaves from the anchor's own edge rather than from `deep`: it is a hand's breadth
                // below the row already, and a long reach across the pane would only be more height
                // for a neighbour's run to cross.
                let from = if at_home { a_x } else { deep };
                let (lane0, step) = match (at_home, left) {
                    (true, true) => (a.at.max.x, -lane),
                    (true, false) => (a.at.min.x, lane),
                    (false, true) => (band_near, lane),
                    (false, false) => (band_far, -lane),
                };
                ties.push((
                    i,
                    Tie { at: Vec2::new(from, a_y), edge: box_edge, mid, lane0, step },
                    a_x,
                    at_home,
                ));
                taken.push(Rect::from_corners(pos, pos + size));
                // **The ground a leader might use, reserved before its lane is known.** Pinned to
                // the whole span any lane could reach rather than to one of them, so whichever lane
                // the pass below settles on, the ground is already spoken for and no later box can
                // land on it.
                // The span any lane of this leader could reach: the band for a rail box, the
                // gutter for a dock one.
                let (turn_lo, turn_hi) = if at_home {
                    (box_edge.min(a_x), box_edge.max(a_x))
                } else {
                    (band_near, band_far)
                };
                for r in [
                    Rect::from_corners(
                        Vec2::new(from.min(turn_lo), a_y - thick),
                        Vec2::new(from.max(turn_hi), a_y + thick),
                    ),
                    Rect::from_corners(
                        Vec2::new(box_edge.min(turn_lo), mid - thick),
                        Vec2::new(box_edge.max(turn_hi), mid + thick),
                    ),
                ] {
                    taken.push(r);
                }
                // **Forward only, and now for the whole side.** `taken` is what stops boxes
                // overlapping; the floor is only the order — and the order is the crossing-free
                // argument, so this is the line that carries it. A box standing in the dock moves
                // the floor the next rail box starts from, and vice versa: that is the point, not
                // an inefficiency to be optimised away.
                floor = floor.max(y + size.y + stack);
            }

            // **Lanes last, from the geometry every box actually ended up with.** A lane is a
            // rendering of the tie, not a constraint on it — nothing above depends on which one a
            // leader gets, which is exactly what lets this run once, knowing all of them.
            // **A side that uses both columns puts every leader's start on its anchor's edge.**
            //
            // Two columns on one side is two corridors: a dock box lanes in the gutter *inside* its
            // anchor's outer edge, a rail box in the band *outside* it. That is safe on its own —
            // the two strips do not overlap — and it is the **deep reach** that breaks it. A rail
            // leader walks inward to land on the label itself, so it starts left of the anchor's
            // edge and crosses the whole gutter on its way out, at whatever height it leaves. No
            // lane a dock leader can take avoids it: `no_two_leaders_cross` reported exactly that
            // the moment a paned control could reach its own dock
            // (`MESHES at 1680x1050: Control(CellGrid)'s leader meets Control(Tags)'s`).
            //
            // Starting every leader at `a_x` makes the two groups **structurally disjoint** — every
            // dock segment lies at or left of the anchor's edge, every rail segment at or right of
            // it — so no assignment has to be searched for and none can fail. The cost is the one a
            // dock leader already pays and for the same reason: it points at the row's edge rather
            // than at the word. A side that uses one column keeps the deep reach.
            let mixed = ties.iter().any(|(_, _, _, home)| *home)
                && ties.iter().any(|(_, _, _, home)| !*home);
            let held: Vec<Tie> = ties
                .iter()
                .map(|(_, t, a_x, _)| {
                    let mut t = *t;
                    if mixed {
                        t.at.x = *a_x;
                    }
                    t
                })
                .collect();
            for (slot, k) in lanes_for(&held, lanes, thick).into_iter().enumerate() {
                let (Some((i, ..)), Some(tie)) = (ties.get(slot), held.get(slot)) else {
                    continue;
                };
                plans[*i].lead = tie.leader(k, thick);
            }
        }
        // The legend last: it has a whole stage to stand in, so it yields to everything.
        for (i, a) in anchors.iter().enumerate() {
            let ((cluster, ..), Some(_)) = (&items[i], a) else {
                continue;
            };
            if cluster.0 != Home::Legend {
                continue;
            }
            let size = items[i].1.size();
            // **`reach`, not `stack`.** The legend is its own block and stands clear of the rails
            // the way it stands clear of a panel; `chrome::MARGIN` is how far it sits off the
            // window's own edge, which is a different measurement and always was.
            plans[i].pos = Some(settle_legend(
                size,
                stage,
                &ground,
                &taken,
                reach,
                chrome::MARGIN * scale,
            ));
        }
        (plans, taken)
    };

    // **What is wrong with an attempt, as one number.**
    //
    // This used to ask one question — *did the legend land on something* — and retry compact only
    // for that. The legend is not a special case: a box standing on a box is the same failure
    // whoever owns it, and on a real kit at 1280x800 it is the **rails** that run out of stage
    // first, so the one arrangement the retry existed to rescue was the one arrangement it never
    // fired for. Measured on the furniture kit: fourteen overlapping pairs, none of them the
    // legend's, and the comfort pass stood because nothing asked it not to.
    //
    // So: every placed box against every other, plus the legend's own collision with the ground and
    // leaders `taken` holds, which is the question the old check asked and is not expressible as a
    // box-versus-box pair.
    let score = |plans: &[Plan], taken: &[Rect]| -> usize {
        let placed: Vec<(usize, Rect)> = (0..items.len())
            .filter_map(|i| {
                plans[i]
                    .pos
                    .map(|at| (i, Rect::from_corners(at, at + items[i].1.size())))
            })
            .collect();
        let pairs = placed
            .iter()
            .enumerate()
            .flat_map(|(k, (_, a))| placed.iter().skip(k + 1).map(move |(_, b)| (a, b)))
            .filter(|(a, b)| covers(**a, **b))
            .count();
        let legend = placed.iter().any(|(i, me)| {
            items[*i].0 .0 == Home::Legend && taken.iter().any(|t| covers(*t, *me))
        });
        pairs + usize::from(legend)
    };

    let (mut plans, taken) = attempt(false);
    let comfort = score(&plans, &taken);
    if comfort > 0 {
        let (again, again_taken) = attempt(true);
        // **Strictly better, or comfort stands.** Compact packs every rail tight from the stage's
        // top, which frees ground but costs every box its place beside its own row — so it has to
        // earn the trade rather than merely tie. A screen that defeats both is what the overlap
        // ratchet is for.
        if score(&again, &again_taken) < comfort {
            plans = again;
        }
    }

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

// **An accent halo on every anchored control was tried here, and removed.**
//
// `light_anchored_controls` gave each control that owned a cluster a one-pixel `ACCENT` outline
// while `K` was held, on the argument that ownership by proximity is deniable in a dock of stacked
// rows. It answered a question the leaders had not yet been built to answer — and once they were,
// it became a second answer competing with the first: reported from the keyboard, *"the yellow-gold
// outlines make it hard to see where the lines are pointing."* Twenty-odd glowing rectangles are
// exactly the wrong ground to read a hairline against.
//
// So ownership is the **leader**, and only the leader — the one tie
// `every_control_cluster_is_tied_to_its_anchor` holds the packer to. Nothing writes `Outline` in
// this crate now; if something ever needs to, note that this system's rest state was `Color::NONE`
// rather than a removal, because `Outline`'s own doc recommends toggling by colour.

#[cfg(test)]
mod tests {
    use super::{FreeGround, CELL};
    use bevy::prelude::*;

    fn window() -> Rect {
        Rect::from_corners(Vec2::ZERO, Vec2::new(640.0, 320.0))
    }

    /// A screen nothing is drawn on is free everywhere, including flush into its corners.
    #[test]
    fn an_empty_screen_is_free_ground_end_to_end() {
        let g = FreeGround::build(window(), &[]);
        assert!(g.is_free(Rect::from_corners(Vec2::ZERO, Vec2::new(640.0, 320.0))));
        assert!(g.is_free(Rect::from_corners(
            Vec2::new(600.0, 300.0),
            Vec2::new(640.0, 320.0)
        )));
    }

    /// **Off the edge is not free.** There is no ground out there to stand on, and a box clamped
    /// back inside is the overlap this module has no give-up arm for.
    #[test]
    fn ground_that_leaves_the_window_is_not_ground() {
        let g = FreeGround::build(window(), &[]);
        assert!(!g.is_free(Rect::from_corners(
            Vec2::new(620.0, 10.0),
            Vec2::new(700.0, 40.0)
        )));
        assert!(!g.is_free(Rect::from_corners(
            Vec2::new(-8.0, 10.0),
            Vec2::new(40.0, 40.0)
        )));
    }

    /// **A cell any ink touches is occupied**, which is the rounding this grid must do: "free" has
    /// to mean genuinely free, or the exact ratchet in `tests/headless.rs` and this grid disagree
    /// and the overlay ships an overlap that the suite calls green.
    #[test]
    fn a_cell_ink_only_grazes_still_counts_as_used() {
        let ink = [Rect::from_corners(
            Vec2::new(100.0, 100.0),
            Vec2::new(101.0, 101.0),
        )];
        let g = FreeGround::build(window(), &ink);
        // The one-pixel mark occupies its whole cell, so a box overlapping any part of that cell is
        // refused — even the part the ink does not literally cover.
        let cell_x = (100.0f32 / CELL).floor() * CELL;
        assert!(!g.is_free(Rect::from_corners(
            Vec2::new(cell_x, 96.0),
            Vec2::new(cell_x + 2.0, 104.0)
        )));
    }

    /// The ordinary case the whole search rests on: a box fits beside an obstacle and not through it.
    #[test]
    fn a_box_finds_the_ground_beside_a_block() {
        let ink = [Rect::from_corners(
            Vec2::new(0.0, 0.0),
            Vec2::new(320.0, 160.0),
        )];
        let g = FreeGround::build(window(), &ink);
        assert!(!g.is_free(Rect::from_corners(
            Vec2::new(280.0, 100.0),
            Vec2::new(400.0, 140.0)
        )));
        // Beside it, one cell clear of the edge.
        assert!(g.is_free(Rect::from_corners(
            Vec2::new(320.0 + CELL, 100.0),
            Vec2::new(440.0, 140.0)
        )));
        // And below it.
        assert!(g.is_free(Rect::from_corners(
            Vec2::new(10.0, 160.0 + CELL),
            Vec2::new(300.0, 300.0)
        )));
    }

    /// Two blocks that overlap must not cancel: the difference array adds and subtracts, and a
    /// sign error there reads as a hole in the middle of solid ink.
    #[test]
    fn overlapping_ink_does_not_cancel_itself_out() {
        let ink = [
            Rect::from_corners(Vec2::new(40.0, 40.0), Vec2::new(200.0, 200.0)),
            Rect::from_corners(Vec2::new(100.0, 100.0), Vec2::new(260.0, 260.0)),
        ];
        let g = FreeGround::build(window(), &ink);
        assert!(!g.is_free(Rect::from_corners(
            Vec2::new(120.0, 120.0),
            Vec2::new(180.0, 180.0)
        )));
        assert!(g.is_free(Rect::from_corners(
            Vec2::new(280.0, 280.0),
            Vec2::new(400.0, 310.0)
        )));
    }
}

/// **One badge, wherever it is going.** The pad and the ordinary flow spawn the same thing; a second
/// spelling of a badge is the drift `chrome.rs` exists to stop, one module along.
fn one_badge(
    c: &mut ChildSpawnerCommands,
    badge: &keys::Badge,
    boxed: bool,
    chord_col: f32,
    does_col: f32,
) {
    c.spawn((
        Badge(badge.actions.clone()),
        Node {
            padding: chrome::CHIP_PAD,
            // **A row inside a box needs no box of its own.** That is now true of every
            // labelled cluster and not just the legend's — see the cluster's own node for
            // why the per-chord border had to go: it drew one box per chord against one
            // leader per cluster, so most boxes pointed at nothing.
            //
            // A bare chord in a band is still its own object, and still carries its own.
            border: if boxed {
                UiRect::ZERO
            } else {
                UiRect::all(Val::Px(chrome::EDGE_W))
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
        BackgroundColor(if boxed { Color::NONE } else { PANEL_BG }),
        BorderColor::all(if boxed { Color::NONE } else { KEY }),
        // The rest state a lit chord returns to. Transparent inside a box, which is what lets
        // `flash_live_badges` still read: an unlit row shows the box's own fill, and a lit one
        // takes `ROW_SELECTED` + `ACCENT` and stands out *within* the box rather than beside it.
        BadgeRest(
            if boxed { Color::NONE } else { PANEL_BG },
            if boxed { Color::NONE } else { KEY },
        ),
        bevy::picking::Pickable::IGNORE,
    ))
    .with_children(|b| {
        b.spawn((
            Node {
                // The chord holds a column so the descriptions line up down the box —
                // `docs/ui.md` §3.1's argument that a panel is rows and not strings, applied to the
                // last thing in this editor that was still a string. Only inside a box: a bare chip
                // in a band is its own object and hugs its chord.
                min_width: if boxed { Val::Px(chord_col) } else { Val::Auto },
                flex_shrink: 0.0,
                ..default()
            },
            Text::new(badge.chord.clone()),
            TextColor(KEY),
            // The chord is the one thing a badge exists to have read, so it gets the reading
            // size — the descriptions stay at `HINT`, quieter on both axes.
            chrome::font(chrome::text::BODY),
            // A chord with a space in it is one token to a reader and two to a
            // line-breaker.
            TextLayout::new(Justify::Left, LineBreak::NoWrap),
            bevy::picking::Pickable::IGNORE,
        ));
        if boxed {
            // **The width lives on a wrapper, not on the text node.**
            //
            // A `Text` measures itself, and a `max_width` on the same node it
            // measures on is applied *after* — so it reported one line's height,
            // the row was built that tall, and the second line drew below the box
            // and through the row beneath it. Reported from the keyboard: *"the
            // box is too small for the text, so the text just slips under it
            // where it can't be seen."* Constrained from outside, the measure
            // runs against the width it will actually get.
            b.spawn((
                Node {
                    // A column, so the block has a width you can predict. Capping the
                    // *cluster* instead was tried and is the wrong lever:
                    // `align_items: Stretch` plus a shrinkable row let flex take the
                    // words down to nothing rather than wrap them.
                    // A cap, not a width: the wrapper hugs its words and wraps only past this, so a
                    // short description costs exactly what it measures. The fixed width before it
                    // made every box as wide as the longest description the census allows.
                    // Passed in rather than branched on here: the legend has a whole stage to stand
                    // in and can afford a wide column, a control's box has to fit on whatever free
                    // ground is left beside it. That is a fact about the *home*, and the caller is
                    // where the home is known.
                    max_width: Val::Px(does_col),
                    ..default()
                },
                // **The wrapper answers the pointer too, or the whole layer does.** It is the one
                // node in this subtree that carries no marker, so an `Or<With<..>>` guard could not
                // see it, and `build_hover_map` blocks on an entity with no `Pickable` by default —
                // a hover stopping here is a hover `view::over_ui` reads as "the pointer is on the
                // interface" everywhere the description happens to be.
                bevy::picking::Pickable::IGNORE,
            ))
            .with_children(|w| {
                w.spawn((
    Text::new(badge.does.to_owned()),
    // Quieter than the chord: the chord is what the eye scans
    // for, the words are what tell it whether to stop.
    TextColor(chrome::DIM),
    chrome::font(chrome::text::HINT),
    TextLayout::new(Justify::Left, LineBreak::WordBoundary),
    bevy::picking::Pickable::IGNORE,
                ));
            });
        }
    });

}
