//! **Assembling a tile** — the Tiles tab.
//!
//! A tile is a floor mesh, a wall over it, a fixture on the wall, maybe a decal. The author walks a
//! grid inside the tile with the keyboard and drops meshes onto it; when it is finished they save it,
//! and the Map places it as one thing.
//!
//! # Why this is a tab and not a mode
//!
//! It was a mode for a day. Bringing a mesh in and arranging meshes looked like the same activity a
//! minute apart, so they shared a tab and a `C` key flipped between them — which worked, and was
//! wrong for a reason the key budget hid: `docs/research/2026-08-08-kitbashing-guidance.md` says *"A
//! good kit is hierarchical: parts -> sub-assemblies -> assemblies"*, the editor already gave a tab
//! to every other level, and only that one carried two.
//!
//! A mesh is a measurement written to `library.ron` and described once; a tile is an arrangement
//! written to `compositions.ron` and built constantly. Splitting them retired the mode key with the
//! mode: a tab strip is a mode nobody can forget, which is Raskin's whole condition. FVS-R-21.
//!
//! The twelve-row cap is what made two contexts affordable —
//! `no_context_carries_more_than_a_learnable_vocabulary`, and Liapis's **user fatigue**, one of whose
//! named causes is *"when there are too many options"*.
//!
//! # The grid is the piece's own span, deepened in thirds
//!
//! The stops a plain arrow walks are **not** the Map's lattice: they divide the span between the
//! tile's centre and the focused piece's flush position — [`aligned`]'s own arithmetic — so flush
//! and centre are exactly reachable at every depth, which no lattice of the tile's own can say
//! (`site/wall` sits flush at 0.45, on no rung of any divisor). `J` deepens the ladder by
//! `policy.snap_divisor` (thirds) and wraps. Asked for at the keyboard, 2026-08-14: *"it starts in
//! the center, left moves it flush left ... press J once, then Left, then it moves between flush
//! (outer grid line) and center."*
//!
//! # Flush falls out; it is not a verb
//!
//! A piece lands with its **minimum corner** on the cell it was dropped in, the same rule
//! `grid::snap_corner` states for the Map. Cell zero's corner *is* the tile's edge, so a 0.1 m wall
//! dropped there has its centre at −0.45 — which is exactly the flush position
//! `docs/2026-08-09-compose-authoring-plan.md` §4 records as *"off the lattice by construction"* and
//! which the deleted `compose::flushed` verb existed to reach. Stating the rule on the corner rather
//! than the centre reaches it without a verb.

use bevy::prelude::*;

use emerge_core::composition::{Body, Composition, Envelope, Member};
use emerge_core::stack;

/// **The tile being assembled**, and where the cursor is in it.
#[derive(Resource, Default)]
pub struct Build {
    /// The tile in hand, or `None` when nothing is being built. Absence is a real state — the tab
    /// opens in `Describe` and an author may never build anything.
    /// **Whether the arrows are steering the tile or the library list.**
    ///
    /// The author asked for this by name — *"get a key to start the placement so the arrows don't
    /// get elsewhere"* — after finding `T F G H` "gross". One set of arrows cannot walk two lists at
    /// once, and the census forbids two actions on one key in one context, so the key is single and
    /// the **state** decides which job it does.
    ///
    /// It is a mode, and I10 of the editor-model guide argues against those. Raskin's objection is
    /// to a mode you can *forget*, and this one is drawn: while placing there is a ghost standing on
    /// the grid under the cursor, and the status line says so. `Esc` leaves.
    pub placing: bool,
    pub open: Option<Composition>,
    /// Which member has focus, as an index into `open`'s members. Out of range reads as "none", which
    /// is what happens when the focused member is dropped.
    ///
    /// **There is no cursor beside it, and that is deliberate.** A cell cursor lived here until it
    /// became a second answer to "where are we": the arrows move the focused member, so the member
    /// *is* the position, and the two disagreed the moment the envelope started fitting its contents.
    /// It was kept on as a derived readout and went stale immediately — written only by the nudge,
    /// while a drop, a removal and an undo all move the focus — and its readers measured it from the
    /// tile's minimum corner while its one writer measured it in signed rungs from the centre. What
    /// the panel shows now is the focused member itself.
    pub focus: usize,
    /// **How deep the arrow ladder is**, latched. `0` walks centre → flush in one press; each level
    /// down divides every interval by `policy.snap_divisor`; `J` wraps past [`DEPTHS`].
    ///
    /// Bier's snap-dragging changes gravity modes with keyboard commands and holds nothing; of its 44
    /// commands the modal ones are all latched. StickyLines says why holding costs: its designers
    /// *"make extensive use of the keyboard … not only because it is faster, but also because 'there
    /// are too many options and menus' that clutter their screens and make them 'lose focus'."*
    /// Holding Shift is right for one nudge and wrong for a dressing session, and a dressing session
    /// is what building a tile is.
    ///
    /// Safe to latch because it is **visible**: the drawn grid redraws at the active depth, so this
    /// is not a mode anyone can forget they are in.
    pub depth: u32,
    /// **How many times a different tile has been opened.**
    ///
    /// A document boundary, as data rather than as a call. `TileHistory` watches the tile rather than
    /// hooking each verb — which is what makes every *mutation* covered by construction — but
    /// "a different tile is open now" is not a mutation, it is a new document, and a stack that
    /// spans two of them makes `Cmd+Z` mean whichever was touched last. `TileHistory`'s own note
    /// already makes that argument about the two *tabs*; this is the same argument one level down.
    pub opened: u32,
    /// **The name being typed for a new tile**, or `None` when nothing is being named.
    ///
    /// Its own field rather than a second condition on `EditorState::grouping`, which is the Map's
    /// composition prompt — `chrome::paint_name_box`'s own doc asks for exactly this: *"if a second
    /// tab ever asks again the answer is another field, not another condition on this one."*
    ///
    /// Tiles used to be named FOR the author (`kit/tile_1`, `tile_2`, …) with no way to say
    /// otherwise, which was tolerable while they were invisible and stopped being so the moment the
    /// KIT list showed them. Asked for at the keyboard, 2026-08-15: naming should be explicit.
    pub naming: Option<NamePrompt>,
    /// **Whether the open tile still carries the name the EDITOR gave it.**
    ///
    /// Set only by [`open_blank`], cleared by every other opener. A fact recorded where it happens,
    /// rather than deduced from the id's shape — the first version matched `tile_<digits>` and was
    /// wrong the moment a tile legitimately called `tile_4` was reopened from disk: it read as
    /// provisional forever, so every save of it asked for a name again. A tile that came off disk is
    /// named by definition, whatever it is called.
    pub provisional: bool,
    /// **Whether the kit list is showing**, and where the cursor is in it.
    ///
    /// `Some(row)` is `Stance::Browsing`. Kept here rather than in a resource of its own because it
    /// is the same kind of fact as `placing` — what the arrows are for — and the stance already reads
    /// this struct to decide.
    pub browsing: Option<usize>,
}

/// **A tile name being typed, and what happens when it is confirmed.**
///
/// The intent is carried rather than inferred. One prompt serves two verbs — `N` names a tile that
/// does not exist yet, `Cmd+S` names one that does — and a first version guessed between them from
/// whether the open tile had members, which silently renamed and saved the tile in hand when the
/// author had asked for a new one. What raised the prompt is a fact; deducing it from state is how
/// two paths end up sharing one answer.
#[derive(Clone, Debug, PartialEq)]
pub struct NamePrompt {
    /// What has been typed, before `to_snake_case`.
    pub raw: String,
    pub then: NameThen,
}

/// Which verb is waiting on the name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameThen {
    /// `N` — open a new blank tile under this name.
    Open,
    /// `Cmd+S` — the open tile has never been named; name it and write it.
    Save,
}

/// The ladder depth a tile opens at: the span itself, so the first press from centre lands flush.
pub const DEFAULT_DEPTH: u32 = 0;

/// How many depths `J` cycles before wrapping — the span, thirds of it, ninths of it.
///
/// Asked for at the keyboard, 2026-08-14: *"press J once for smaller grid, then press J again for
/// even smaller grid, and a third press would reset to original."*
pub const DEPTHS: u32 = 3;

/// A position within this of a ladder stop **is** that stop — the millimetre `touching` already
/// uses, and for the same reason: authored numbers come out of a DCC, not out of arithmetic.
pub const ON_STOP: f32 = 1e-4;

/// **A tile member stands up**, because [`Member`] records no tip.
///
/// [`emerge_core::map::Placed`] carries a `tip` and a member does not, so every footprint question
/// asked about a member here answers for an upright piece. That is the schema's answer rather than
/// a guess — and it is stated once, under a name, so the day `Member` grows a tip the seven call
/// sites that would need to change are the seven this constant appears at. A bare `(0, 0)` at each
/// of them would be indistinguishable from a caller that simply forgot, which is exactly how the
/// tip went missing from both fills.
const MEMBERS_STAND_UP: (u8, u8) = (0, 0);

/// An empty tile of the standard size, ready to take members.
///
/// One cell across and `height` tall, because that is what a tile *is* — `grammar::from_compositions`
/// refuses a `Bounded` composition that is not exactly the grid's size, so a tile of any other width
/// is a group the solver can never place.
pub fn blank(id: &str, height: f32) -> Composition {
    Composition {
        id: id.to_owned(),
        envelope: Envelope::Bounded {
            size: (emerge_core::grid::TILE, height, emerge_core::grid::TILE),
        },
        members: Vec::new(),
        locations: Vec::new(),
        note: None,
    }
}

/// **Where a member sits when it is flush against one side of the tile.**
///
/// The position is a function of the piece's **own width**, which is why it is a verb rather than a
/// place the arrows can reach: `site/wall` is 0.1 m thick and sits flush at -0.45 in a 1 m tile, and
/// no rung of any divisor lands on -0.45. `policy.rs` already wrote this down about seating —
/// *"which is not a multiple of 0.125 either, because art is authored to look right rather than to
/// tile"* — and the answer there was the same: ask for the edge, do not step to it.
///
/// Only the pressed axis moves. Shifting a piece to the left edge should not also recentre it front
/// to back, and a second axis moving on its own is the kind of surprise that makes an author stop
/// trusting a key.
pub fn aligned(
    at: (f32, f32),
    span: (f32, f32),
    size: (f32, f32, f32),
    dir: (i32, i32),
) -> (f32, f32) {
    let mut out = at;
    if dir.0 != 0 {
        out.0 = flush_reach(size.0, span.0) * dir.0 as f32;
    }
    if dir.1 != 0 {
        out.1 = flush_reach(size.2, span.1) * dir.1 as f32;
    }
    out
}

/// **How far a piece's centre can travel from the tile's centre before it is flush** — half the
/// free space on that axis.
///
/// The one expression [`aligned`] and the arrow ladder both read, so the flush verb and the
/// ladder's outermost stop are the same number **by construction** rather than by two float
/// expressions that happen to agree — `size*0.5 - span*0.5` and `(size - span)*0.5` are not the
/// same rounding in f32, and a piece flush by one verb and a ULP off by the other would put the
/// "already at the flush stop" answer a press away from the truth.
pub fn flush_reach(size_axis: f32, span_axis: f32) -> f32 {
    size_axis * 0.5 - span_axis * 0.5
}

/// **The envelope that holds what is in the tile** — whole cells, centred, never smaller than one.
///
/// The author's model: *"as many whole tiles as needed to capture the object… if the mesh is
/// adjusted and tiles are no longer needed, they're automatically removed… if it falls on the seam,
/// more tiles are added."* So the envelope is not a thing to set, it is a thing that is **read off
/// the contents** — the same rule `Interface` follows, and for the same reason: a size that is
/// authored separately from the members it is supposed to contain is a second source of truth about
/// the same fact, and the two drift.
///
/// **Centred, because that is what the envelope already means.** Members position relative to the
/// composition's anchor and `validate` measures a slot against `±size/2`, so growing asymmetrically
/// would move every existing member relative to the box. A piece 1.21 m across therefore needs two
/// cells, not one-and-a-bit: it reaches 0.605 from the anchor and one cell only reaches 0.5.
///
/// **Height is not fitted.** A tile is as tall as the space it sits in — `stack::datum` records what
/// hardcoding a ceiling height costs — so the vertical stays whatever the map declares.
pub fn fit_envelope(
    members: &[Member],
    library: &emerge_core::library::Library,
    height: f32,
) -> (f32, f32, f32) {
    let tile = emerge_core::grid::TILE;
    let mut reach = (0.0f32, 0.0f32);
    for m in members {
        // A hole has no mesh and so no footprint; it is a point, and `validate` already refuses one
        // outside the envelope. Its position still counts toward the reach.
        //
        // **And it counts a hair further than it stands**, because `validate_shape` requires a slot
        // to be *strictly* inside — *"a slot exactly on the seam is the ambiguous case"* — while a
        // zero span makes the envelope come out at exactly `2·|at|`, putting the hole on the seam by
        // construction. One nudge east of centre at a divisor of 2 was enough: the tile then refused
        // to save at all, naming an envelope the author never chose and could only escape by
        // guessing to nudge back. The margin has to exceed the slack below or the `ceil` swallows
        // it; a millimetre is the same skin `touching` uses, for the same reason.
        const OFF_THE_SEAM: f32 = 1e-3;
        let span = match &m.body {
            Body::Descriptor { id, .. } => library
                .get(id)
                .map(|d| crate::editor::brush_span(d, m.yaw, MEMBERS_STAND_UP))
                .unwrap_or((0.0, 0.0)),
            Body::Slot { .. } => (OFF_THE_SEAM * 2.0, OFF_THE_SEAM * 2.0),
            _ => (0.0, 0.0),
        };
        reach.0 = reach.0.max((m.at.0.abs() + span.0 * 0.5).abs());
        reach.1 = reach.1.max((m.at.1.abs() + span.1 * 0.5).abs());
    }
    // A hair of slack, or a piece measured at exactly one cell buys a second one on float noise.
    let cells_for = |r: f32| (((2.0 * r) / tile) - 1e-4).ceil().max(1.0);
    (cells_for(reach.0) * tile, height, cells_for(reach.1) * tile)
}

/// How many cells the envelope divides into at this rung, never zero.
///
/// Y divides on the same pitch as X and Z rather than on its own: *"up one"* and *"across one"* being
/// different distances is the confusion the single ladder exists to remove, and a tile 2.4 m tall on a
/// third-metre rung is seven layers, which is a number an author can hold.
pub fn cells(size: (f32, f32, f32), pitch: f32) -> (u32, u32, u32) {
    let n = |v: f32| ((v / pitch).round() as i64).clamp(1, i64::from(u32::MAX)) as u32;
    (n(size.0), n(size.1), n(size.2))
}

/// **A cell step in the frame the author is looking at it from**, which is why it is a diagonal.
///
/// The camera sits on `ISO_OFFSET = (12, 12, 12)`, so screen-up is the world direction
/// `(-0.707, 0, -0.707)` — a diagonal in cell space. Stepping along a world axis therefore *looks*
/// diagonal, which is what the author hit: *"the arrow keys should move in diagonals."* Read the
/// other way round, they were asking for the arrows to mean what they point at.
///
/// `view::pan_direction` already turns a screen wish into a world direction and is tested at every
/// rotation detent, so this borrows it rather than deriving a second answer — and that is what makes
/// the step follow the camera when the author turns it, instead of being right only at the default
/// framing.
///
/// **One axis per press, and four presses mean four different axes.**
///
/// Two goes at this were wrong in instructive ways. The first stepped *both* axes whenever both
/// projected components were significant, which at the iso yaw is always — so the cursor jumped to
/// the diagonally-adjacent square and skipped the one beside it. The second took the dominant
/// component, and at the iso yaw there is no dominant component: screen-up is world
/// `(-0.707, -0.707)` and screen-left is `(-0.707, +0.707)`, equal magnitudes, so the tiebreak sent
/// **both** to `-x` and two of the four arrows did nothing. That was invisible only because the
/// camera had been turned square-on at the same time, which the author then quite reasonably
/// objected to: *"I just wanted the keys to go a different direction, not rotate the whole visual."*
///
/// So: take the **angle** of the world direction and snap it to the nearest quarter turn. Four
/// screen directions ninety degrees apart land on four world axes ninety degrees apart, whatever the
/// camera is doing — the mapping rotates with the view instead of the view rotating for it.
///
/// The bias is what makes that true at the iso yaw specifically, where every press lands exactly on
/// a quadrant boundary. A hair of rotation before rounding sends the two neighbours of each boundary
/// to different sides of it, which is the difference between four working arrows and two. It is
/// stated rather than left to `f32` rounding because "which way does `round` break a tie" is not a
/// thing this should depend on.
pub fn step_in_view(wish: Vec2, yaw: f32) -> (i32, i32) {
    const OFF_THE_BOUNDARY: f32 = 1e-3;
    const AXES: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];
    let d = crate::view::pan_direction(wish, yaw);
    if d.x == 0.0 && d.z == 0.0 {
        return (0, 0);
    }
    let turns = (d.z.atan2(d.x) + OFF_THE_BOUNDARY) / std::f32::consts::FRAC_PI_2;
    AXES[(turns.round() as i32).rem_euclid(4) as usize]
}

/// A member id that is not already taken, derived from the piece's own name.
///
/// `site/wall` becomes `wall`, then `wall_2`, `wall_3`. Derived rather than typed because naming
/// every member by hand is the kind of friction that makes an author stop using a tool — and the id
/// only has to be unique and stable, which a suffix achieves.
pub fn fresh_id(members: &[Member], descriptor: &str) -> String {
    let short = descriptor.rsplit('/').next().unwrap_or(descriptor);
    let taken = |id: &str| members.iter().any(|m| m.id == id);
    if !taken(short) {
        return short.to_owned();
    }
    // Bounded by the member count plus one, so this cannot spin: at most `n + 1` names can be taken.
    for n in 2..=members.len() + 2 {
        let candidate = format!("{short}_{n}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    short.to_owned()
}

/// **A member id for a hole, derived from the vocabulary token it accepts.**
///
/// Separate from [`fresh_id`] because the two have different preconditions, and conflating them is
/// what broke: `fresh_id` assumes its seed is already a legal id segment, which is true of a
/// descriptor id (`naming::is_id` validates every one on the way into `library.ron`) and **false of
/// a vocabulary token**. Tokens are deliberately not ids — `vocab.rs` documents
/// `"uses-electricity"` and `"stamina-recharge"` — so seeding a member id with `wall-fixture`
/// produced a member `composition::validate` refuses, and a tile carrying any hole could not be
/// saved at all. Both slot tokens the project actually declares are hyphenated, so on the real
/// project this was every hole.
///
/// The conversion is checked against `naming::is_id` rather than assumed correct by construction,
/// so it cannot drift from the rule it is trying to satisfy. A token that cannot yield an id —
/// one starting with a digit, say — is **refused by name** rather than silently renamed to
/// something that happens to parse.
pub fn slot_id(members: &[Member], accepts: &str) -> Result<String, String> {
    let mut seed = String::with_capacity(accepts.len());
    for c in accepts.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            seed.push(c);
        } else if !seed.is_empty() && !seed.ends_with('_') {
            seed.push('_');
        }
    }
    while seed.ends_with('_') {
        seed.pop();
    }
    if !emerge_core::naming::is_id(&seed) {
        return Err(format!(
            "slot token `{accepts}` cannot name a member — ids start with a lowercase letter and              carry only letters, digits and `_`. Rename the token in vocab.ron."
        ));
    }
    Ok(fresh_id(members, &seed))
}

/// **Members stay sorted by id**, which is what `composition::validate` holds every group to so that
/// one group has one encoding. Called after every insertion rather than at save, so the list an
/// author reads is the list that will be written.
pub fn insert_sorted(members: &mut Vec<Member>, m: Member) -> usize {
    let at = members.partition_point(|o| o.id < m.id);
    members.insert(at, m);
    at
}

/// Two plan boxes touch — overlapping, or meeting within a hair.
///
/// A local epsilon because `adjacency::EDGE_EPSILON` is `pub(crate)` to `emerge-core`, and the same
/// millimetre for the same reason: art is authored to look right, so a panel and the fixture on it
/// meet at a number that came out of a DCC rather than out of arithmetic.
fn touching(a: ((f32, f32), (f32, f32)), b: ((f32, f32), (f32, f32))) -> bool {
    const SKIN: f32 = 1e-3;
    let span1 = |c: f32, s: f32| (c - s * 0.5, c + s * 0.5);
    let (ax0, ax1) = span1(a.0.0, a.1.0);
    let (az0, az1) = span1(a.0.1, a.1.1);
    let (bx0, bx1) = span1(b.0.0, b.1.0);
    let (bz0, bz1) = span1(b.0.1, b.1.1);
    ax0 <= bx1 + SKIN && bx0 <= ax1 + SKIN && az0 <= bz1 + SKIN && bz0 <= az1 + SKIN
}

/// **Which member a fixture is mounted on, decided by where it was dropped.**
///
/// `Body::Descriptor::on` names *"a sibling `Member::id` this rests on"*, and the assembler wrote
/// `None` into it for every piece — which means "find a host outside this group". A piece with a
/// `Mount::OnFace`/`OnSurface` and no host is refused by `stack::resolve_y`, and `emerge-bevy`
/// propagates that, so **the map refuses to load**. The author's own description of this tab —
/// *"a wall mesh over it, and wall mounted light fixture on the wall mesh"* — was the one clause of
/// it that could not be authored (FVS-R-24).
///
/// Automatic rather than a verb, which was the author's call: the requirement for this loop is the
/// keyboard, and a fixture dropped against a wall has already said which wall it means by being
/// there. Where it has *not* said — two walls equally adjacent — this **refuses naming both** rather
/// than picking the first in some order, which would be a silent decision that ECS query order or a
/// sort could quietly change later.
///
/// Returns `Ok(None)` for a piece that needs no host, which is most of them.
///
/// `self_id` is the guest's own member id when it is already in `members`, so a piece is never
/// offered itself — `validate` refuses a member resting on itself, and a wall that offers the very
/// face it is being asked about would otherwise host itself the moment [`rebind_hosts`] re-asked.
pub fn host_for(
    members: &[Member],
    library: &emerge_core::library::Library,
    guest: &emerge_core::descriptor::Descriptor,
    self_id: Option<&str>,
    at: (f32, f32),
    span: (f32, f32),
) -> Result<Option<String>, String> {
    let Some(want) = crate::editor::mount_class(guest) else {
        return Ok(None);
    };

    let mut found: Vec<&str> = Vec::new();
    for m in members {
        if self_id == Some(m.id.as_str()) {
            continue;
        }
        let Body::Descriptor { id, .. } = &m.body else {
            // A hole offers nothing — it has no mesh yet — and a nested group's faces belong to its
            // own members. Neither is a host.
            continue;
        };
        let Some(host_d) = library.get(id) else {
            continue;
        };
        if !stack::offers_for(host_d, guest) {
            continue;
        }
        let host_span = crate::editor::brush_span(host_d, m.yaw, MEMBERS_STAND_UP);
        if touching((at, span), (m.at, host_span)) {
            found.push(&m.id);
        }
    }

    match found.as_slice() {
        [] => {
            // **Name what would work.** The refusal knew what was missing and not what to do about
            // it, so an author was left to guess which of seventy-five rows offers a `support` —
            // and the editor has that list. `docs/2026-08-11-editor-visual-inspection.md` records
            // this exact shape as D2: *"The information exists; only the channel is missing."*
            //
            // Three, then a count. A refusal naming twenty pieces is not read, and the three are
            // sorted so the same tile refuses the same way on every machine.
            let mut hosts: Vec<&str> = library
                .descriptors
                .iter()
                .filter(|d| stack::offers_for(d, guest))
                .map(|d| d.id.as_str())
                .collect();
            hosts.sort_unstable();
            let offer = match hosts.as_slice() {
                [] => format!(
                    " Nothing in this kit offers a `{want}`, so a hole is the only way to place it."
                ),
                some => {
                    let named = some
                        .iter()
                        .take(3)
                        .map(|i| format!("`{i}`"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let rest = some.len().saturating_sub(3);
                    if rest == 0 {
                        format!(" {named} offers one.")
                    } else {
                        format!(" {named} and {rest} more offer one.")
                    }
                }
            };
            Err(format!(
                "`{}` mounts to a `{want}` and nothing in this tile offers one.{offer} Drop that \
                 first, or press Shift+Enter for a hole the generator fills.",
                guest.id
            ))
        }
        [one] => Ok(Some((*one).to_owned())),
        many => Err(format!(
            "`{}` touches {} — move it against one of them, so the tile says which it is mounted on.",
            guest.id,
            many.iter()
                .map(|i| format!("`{i}`"))
                .collect::<Vec<_>>()
                .join(" and ")
        )),
    }
}

/// **Where a piece lands when it is brought in** — the author's *"bottom line in the center"*.
///
/// A brought-in mesh is not positioned, it is *introduced*; where it goes is the next act, and the
/// arrows do it. Corner-aligning to a cursor cell instead had a second cost that only showed up once
/// the envelope started fitting its contents: a 1 m floor dropped in the middle cell reaches 0.833
/// from the anchor, so the tile grew to 2 x 2 to hold a piece that is exactly one tile.
///
/// **A constant rather than a literal in each of the three places that need it**, because the ghost
/// is one of them. `drive_build_preview` drew its preview through cell-corner arithmetic left over
/// from the deleted cursor while the drop landed here, so the two were on different lattices: a
/// 0.1 m wall previewed flush against the west edge and landed 450 mm away, dead centre, in a tile
/// one metre across. This module's opening line is *"the ghost is the contract"*; one value is what
/// makes that structural rather than a thing to keep in step.
pub const BROUGHT_IN: ((f32, f32), f32) = ((0.0, 0.0), 0.0);

/// **Which sibling each fixture is mounted on, re-read from where everything now stands.**
///
/// One owner, called after every verb — the shape [`refit`] has, and for the same reason. `on` was
/// written once, at drop time, and never looked at again: nudging a wall, flushing it against an
/// edge or turning it left the sconce still claiming to be mounted on it. `composition::validate`
/// only checks that the named sibling *exists*, so that tile saved, and `stack::resolve_y` then put
/// the fixture at its face height with nothing under it — floating in mid-air, in the editor and in
/// the game alike. A binding written once about a relationship the positions decide is a second
/// source of truth, and the two drift the moment anything moves.
///
/// Refusing is the point of the `Result`. A move that would leave a fixture with no host, or with
/// two equally adjacent, is refused by the verb that made it rather than written and discovered at
/// save — the same door `place` has always kept, now kept by every verb.
pub fn rebind_hosts(
    comp: &mut Composition,
    library: &emerge_core::library::Library,
) -> Result<(), String> {
    // Positions as they now stand, so each member resolves against the others' current places rather
    // than against a list being rewritten under it.
    let standing = comp.members.clone();
    for m in comp.members.iter_mut() {
        let Body::Descriptor { id, on, .. } = &mut m.body else {
            continue;
        };
        // A member naming a descriptor the library does not carry cannot be measured, so there is
        // nothing to resolve against and nothing to say — `expand` refuses it by name at stamp time.
        let Some(guest) = library.get(id) else {
            continue;
        };
        let span = crate::editor::brush_span(guest, m.yaw, MEMBERS_STAND_UP);
        *on = host_for(&standing, library, guest, Some(&m.id), m.at, span)?;
    }
    Ok(())
}

/// **The one door every edit to the open tile goes through.**
///
/// Apply the change to a copy, re-resolve the hosts, and keep it only if the result still stands up.
/// A refusal therefore leaves the tile exactly as it was rather than needing the edit undone — which
/// is what `place` used to achieve for the drop alone by resolving before the member existed, and
/// what the nudge, the flush, the turn and the removal each did not. Deleting a wall left the sconce
/// on it naming a member that no longer existed, and `validate_shape` then refused the whole
/// composition with no verb able to repair it.
fn edit<T>(
    build: &mut Build,
    library: &emerge_core::library::Library,
    act: impl FnOnce(&mut Composition) -> Result<T, String>,
) -> Result<T, String> {
    let Some(open) = build.open.as_ref() else {
        return Err("no tile open — press N to start one".to_owned());
    };
    let mut next = open.clone();
    let out = act(&mut next)?;
    rebind_hosts(&mut next, library)?;
    build.open = Some(next);
    Ok(out)
}

/// Whether the arrows have anything to act on — a member under the focus.
///
/// Asked *before* [`edit`] by every verb that moves one, because "the tile is empty" is guidance and
/// everything the door refuses is a refusal. A note is replaced by the next keystroke; a problem is
/// sticky until `Esc`, and an author who pressed an arrow on a tile they have not put anything in
/// yet has not done anything wrong.
pub fn focused(build: &Build) -> bool {
    build
        .open
        .as_ref()
        .is_some_and(|c| build.focus < c.members.len())
}

/// Bring a descriptor into the tile, returning the index it landed at.
///
/// The envelope is **not** adjusted here — `refit` owns that, so growing and shrinking happen in one
/// place whatever changed the members. Nor is `on`: [`rebind_hosts`] owns that, so a fixture's host
/// is decided by where things stand rather than by which verb happened to touch it last.
pub fn place(
    comp: &mut Composition,
    guest: &emerge_core::descriptor::Descriptor,
) -> Result<usize, String> {
    let Envelope::Bounded { .. } = comp.envelope else {
        return Err(format!(
            "`{}` claims no tile, so it has no grid to drop into",
            comp.id
        ));
    };
    let (at, lift) = BROUGHT_IN;
    let m = Member {
        id: fresh_id(&comp.members, &guest.id),
        body: Body::Descriptor {
            id: guest.id.clone(),
            tip: (0, 0),
            on: None,
            patch: None,
        },
        at,
        yaw: 0.0,
        lift,
        paint: 0,
        of_fingerprint: None,
        note: None,
    };
    Ok(insert_sorted(&mut comp.members, m))
}

/// Bring a **hole** into the tile — a position that says what may go here without saying what does.
/// Same gesture, same grid, same keys; see [`Body::Slot`].
pub fn place_slot(comp: &mut Composition, accepts: &str) -> Result<usize, String> {
    let Envelope::Bounded { .. } = comp.envelope else {
        return Err(format!(
            "`{}` claims no tile, so it has no grid to drop into",
            comp.id
        ));
    };
    // **A hole lands in the middle like anything else**, and is moved the same way afterwards.
    //
    // It used to take a cell corner, under a comment reasoning that "the last cell's corner is
    // inside by less than a rung". That was true of the last cell and false of the first: **cell
    // zero's corner is the tile edge**, and `validate` requires a slot to be strictly inside the
    // envelope — so a hole dropped anywhere in row or column zero was refused, including the cell
    // the cursor started in. The first thing an author would try.
    //
    // The centre is interior by construction rather than by luck, and it is the honest position
    // anyway: a slot marks *where* a hole is, and whatever fills it brings its own footprint and its
    // own `Mount` to resolve against. The lift stays on the floor, which is a legal datum (`0.0` is
    // inside) and the one a fixture mounts from.
    let (at, lift) = BROUGHT_IN;
    let m = Member {
        id: slot_id(&comp.members, accepts)?,
        body: Body::Slot {
            accepts: accepts.to_owned(),
        },
        at,
        yaw: 0.0,
        lift,
        paint: 0,
        of_fingerprint: None,
        note: None,
    };
    Ok(insert_sorted(&mut comp.members, m))
}

/// **Save the tile in hand**, and keep it open so the author can carry on.
///
/// Through [`crate::project::Project::commit_composition`] — the one door — so a tile assembled here
/// and a tile captured from a box on the Map are validated and written by the same sequence.
/// `tests/compose_is_read_only.rs` holds that.
///
/// # It stays open on success
///
/// Saving is not finishing. The loop this tab exists to shorten is *drop, look, adjust, save, look* —
/// Compton's grokloop, which Lai et al.'s second pillar asks to keep short — and clearing the tile on
/// save would put a "start again" step in the middle of it. Replacing an existing tile is how a tile
/// is *edited*, which is the same rule capture-over-a-name follows on the Map.
pub fn save(build: &Build, project: &mut crate::project::Project) -> Result<String, String> {
    let Some(comp) = build.open.as_ref() else {
        return Err("no tile open — nothing to save".to_owned());
    };
    if comp.members.is_empty() {
        // The refusal `Composition::validate` would give anyway, said in the author's terms and
        // before the file is touched: an empty tile stamps nothing, which looks like a stamp that
        // failed.
        return Err(format!("`{}` has nothing in it yet", comp.id));
    }
    let existed = project
        .compositions
        .compositions
        .iter()
        .any(|c| c.id == comp.id);
    project.commit_composition(comp.clone())?;
    Ok(if existed {
        format!("`{}` updated — {} members", comp.id, comp.members.len())
    } else {
        format!(
            "`{}` saved — {} members, and it is in the Map palette now",
            comp.id,
            comp.members.len()
        )
    })
}

/// **The stop a plain arrow reaches next** on one axis of the span ladder.
///
/// The ladder divides the span between the tile's centre and the flush position [`aligned`] computes
/// — `f = (size - span) / 2` — into `divisor^depth` steps per side, so **flush and centre are stops
/// at every depth**; no lattice of the tile's own can say that (`site/wall` sits flush at 0.45, on
/// no rung of any divisor). An off-ladder start lands ON the ladder first, which is what keeps a
/// hand-edited or reopened tile walkable rather than stranded a millimetre beside every stop. The
/// ladder ends at flush: a press outward there returns the position unchanged, and the caller says
/// so instead of looking like a dead key.
///
/// The terminal stops return `±f` **exactly** rather than `n * (f / n)`, because the whole point of
/// the ladder is that its outer stop and [`aligned`]'s answer are the same number, not the same
/// number up to float rounding.
///
/// Asked for at the keyboard, 2026-08-14: *"it starts in the center, left moves it flush left ...
/// press J once, then Left, then it moves between flush (outer grid line) and center."*
pub fn ladder_step(pos: f32, f: f32, divisor: u32, depth: u32, dir: i32) -> f32 {
    if f <= ON_STOP || dir == 0 {
        return pos;
    }
    let n = divisor.saturating_pow(depth).max(1) as f32;
    let step = f / n;
    let k = pos / step;
    let k = if (pos - k.round() * step).abs() < ON_STOP {
        k.round() + dir as f32
    } else if dir > 0 {
        k.ceil()
    } else {
        k.floor()
    };
    let k = k.clamp(-n, n);
    if k == n {
        f
    } else if k == -n {
        -f
    } else {
        k * step
    }
}

/// The lift step at this depth, in metres — sub-cell from the start (a third, a ninth, a
/// twenty-seventh of a cell), because "up one whole tile" is a storey, not an adjustment.
pub fn lift_pitch(divisor: u32, depth: u32) -> f32 {
    emerge_core::grid::TILE / divisor.saturating_pow(depth + 1).max(1) as f32
}

/// **Resize the tile to whatever is in it, and keep the cursor inside.**
///
/// One owner, called after the verbs rather than inside each of them, so growing and shrinking
/// happen in the same place whatever changed the members — a drop, a hole, a turn, a removal. Doing
/// it per-verb would be four copies of a rule that has to agree with itself.
///
/// **Writes only on a real change**, because `Build` is a change-detected resource that several
/// systems key their redraws off: assigning an identical size every frame would have the 3-D stage
/// rebuilding forever.
pub fn refit(
    build: &mut Build,
    library: &emerge_core::library::Library,
    height: f32,
) -> Option<(f32, f32, f32)> {
    let Some(comp) = build.open.as_ref() else {
        return None;
    };
    let want = fit_envelope(&comp.members, library, height);
    let now = match comp.envelope {
        Envelope::Bounded { size } => size,
        Envelope::Anchored => return None,
    };
    // **All three axes, because all three are written.** The guard compared X and Z only while the
    // value it writes carries `want.1` as well, so a tile opened under a 2.4 m map and then asked to
    // live under a 3.5 m one kept the old height for ever — and a ceiling fixture inside it came out
    // 1.1 m below the ceiling once stamped. That is the very failure `blank`'s "as tall as the space
    // it fills" and `stack::datum` are written against, reintroduced by an incomplete comparison.
    if (want.0 - now.0).abs() < 1e-4
        && (want.1 - now.1).abs() < 1e-4
        && (want.2 - now.2).abs() < 1e-4
    {
        return None;
    }
    if let Some(comp) = build.open.as_mut() {
        comp.envelope = Envelope::Bounded { size: want };
    }

    Some(want)
}

/// **How many whole tiles across a group is**, which is what decides whether a solver can place it.
///
/// `grammar::from_compositions` takes tiles of the grid's size and *skips* anything else by name,
/// because `solve` lays prototypes at cell centres and a group that is not one cell across would be
/// placed at a spacing with nothing to do with its extent.
pub fn tiles_across(size: (f32, f32, f32)) -> (i32, i32) {
    let tile = emerge_core::grid::TILE;
    (
        (size.0 / tile).round() as i32,
        (size.2 / tile).round() as i32,
    )
}

/// **Is this a size a solver can place?** One cell, both ways.
pub fn is_one_cell(size: (f32, f32, f32)) -> bool {
    tiles_across(size) == (1, 1)
}

/// **Which member made the tile too big, and what it cost.**
///
/// The size line said `1 x 3 tiles — hand-stamped, too big to generate`, which is a consequence with
/// no cause: an author with six members in the tile had no way to learn which one did it, and the
/// answer was one wall nudged 0.67 m off centre. Found by an author standing in front of it during a
/// guided run, not by a test — every test asserted the *count*, which was right.
///
/// The doubling is the part nobody guesses. [`fit_envelope`] measures `|offset| + span/2` and the
/// envelope is **centred on the anchor**, so it has to reach that far on *both* sides: two thirds of a
/// metre off centre buys a metre and a third of tile. Stating the offset alone would still leave the
/// arithmetic to be inferred, so this states what it costs.
///
/// Returns `None` when the tile is one cell, or when the reach is owned by a piece that is simply
/// bigger than a cell — a 2 m sofa is not off centre and there is nothing to nudge, so naming an
/// offset of zero would be worse than saying nothing.
pub fn what_made_it_big(
    members: &[Member],
    library: &emerge_core::library::Library,
    size: (f32, f32, f32),
) -> Option<String> {
    if is_one_cell(size) {
        return None;
    }
    let tile = emerge_core::grid::TILE;
    // The axis that is over, and by how far each member reaches along it. Both may be over; name the
    // worse one, because fixing it is the step the author takes next and the other will still be
    // stated by the count.
    let (nx, nz) = tiles_across(size);
    let axis_x = nx > 1 && nx >= nz;
    let mut worst: Option<(f32, f32, &str)> = None;
    for m in members {
        let span = match &m.body {
            Body::Descriptor { id, .. } => library
                .get(id)
                .map(|d| crate::editor::brush_span(d, m.yaw, MEMBERS_STAND_UP))
                .unwrap_or((0.0, 0.0)),
            _ => (0.0, 0.0),
        };
        let (at, half) = if axis_x {
            (m.at.0.abs(), span.0 * 0.5)
        } else {
            (m.at.1.abs(), span.1 * 0.5)
        };
        let name = match &m.body {
            Body::Descriptor { id, .. } => id.as_str(),
            Body::Slot { .. } => "a hole",
            _ => "a member",
        };
        if worst.is_none_or(|(r, _, _)| at + half > r) {
            worst = Some((at + half, at, name));
        }
    }
    let (_, off, name) = worst?;
    // Centred pieces are not the author's mistake; a piece wider than a cell is a fact about the mesh.
    if off * 2.0 < tile * 0.1 {
        return None;
    }
    let axis = if axis_x { "X" } else { "Z" };
    Some(format!(
        "`{name}` sits {off:.2} m off centre in {axis}, and the tile is centred on its anchor — so \
         that costs {:.2} m. Arrow it back toward the middle.",
        off * 2.0
    ))
}

/// How many steps the tile assembler remembers.
///
/// The same 64 the mesh tab keeps, and bounded for the same reason: a snapshot is a whole
/// composition, and an unbounded stack grows with nothing ever freeing it.
pub const BUILD_HISTORY: usize = 64;

/// **The tile assembler's undo stack.**
///
/// Its own resource rather than fields on [`Build`], because `Build` is change-detected and several
/// systems key their redraws off it — pushing a snapshot would rebuild the 3-D stage every time the
/// history moved, which is the one thing a history should never do.
///
/// Separate from the mesh tab's stack, which is over `library.ron` edits. Two tabs editing different
/// files through one stack would make "undo" mean whichever thing was touched last.
#[derive(Resource, Default)]
pub struct TileHistory {
    past: Vec<Option<Composition>>,
    future: Vec<Option<Composition>>,
    /// What the tile looked like when this system last ran — the thing a change is measured against.
    seen: Option<Option<Composition>>,
    /// Which opened tile the stacks belong to — see [`Build::opened`].
    opened: u32,
    /// **Which member the top of `past` is a run of adjustments to**, or `None` when it is anything
    /// else. See [`adjusted_member`] for why a run is one step.
    run: Option<usize>,
}

/// **Which single member two tiles differ by, if they differ only by adjusting one.**
///
/// `Some(i)` when both hold the same members in the same order and exactly one of them has moved,
/// lifted or turned. `None` for anything else — a drop, a removal, a hole, a resize, or a change to
/// more than one member.
///
/// This is the classifier the undo grouping rests on, and it is deliberately narrow: it answers
/// *"is this the same act continuing"*, and a wrong `Some` would swallow an edit an author wanted
/// back.
fn adjusted_member(before: &Option<Composition>, after: &Option<Composition>) -> Option<usize> {
    let (a, b) = (before.as_ref()?, after.as_ref()?);
    // **The envelope is deliberately not compared.** It is *derived* from the members by
    // `fit_envelope`, and `refit` runs before this system exactly so that a resize is part of the
    // same step as the edit that caused it. Comparing it broke every run that mattered: nudging a
    // piece toward the edge grows the tile, so the classifier called each tap a different kind of
    // act and pushed a fresh entry — the coalescing was there and never once applied.
    if a.id != b.id || a.members.len() != b.members.len() {
        return None;
    }
    let mut moved = None;
    for (i, (x, y)) in a.members.iter().zip(b.members.iter()).enumerate() {
        if x == y {
            continue;
        }
        // Same piece, different placement — anything else (a different body or id) is a new member
        // wearing an old index, which is a drop or a swap rather than an adjustment.
        if x.id != y.id || x.body != y.body {
            return None;
        }
        if moved.is_some() {
            return None;
        }
        moved = Some(i);
    }
    moved
}

/// **What one history step did, in the fewest words that stay true.**
///
/// Undo used to answer `"undo — 2 in the tile"`, which says how many are left and not what left. That
/// is ambiguous exactly when it matters: `place` uses `insert_sorted`, so the MEMBERS list is in **id
/// order, not the order you dropped things** — bring in `zulu` and then `alfa` and the panel shows
/// `alfa` on top, so a correct undo of the most recent drop looks like it threw out the first mesh.
/// The author read it that way, and the mechanism was right the whole time.
///
/// Naming the piece removes the ambiguity without touching the ordering, which exists so the file is
/// the same on every machine.
fn step_says(from: &Option<Composition>, to: &Option<Composition>) -> String {
    let names = |c: &Option<Composition>| -> Vec<String> {
        c.as_ref()
            .map(|c| c.members.iter().map(|m| m.id.clone()).collect())
            .unwrap_or_default()
    };
    let (was, now) = (names(from), names(to));
    let gone: Vec<&String> = was.iter().filter(|n| !now.contains(n)).collect();
    let came: Vec<&String> = now.iter().filter(|n| !was.contains(n)).collect();
    match (gone.as_slice(), came.as_slice()) {
        ([], []) => match adjusted_member(from, to) {
            // The members are the same, so this step is a move — name the piece that moved.
            Some(i) => to
                .as_ref()
                .and_then(|c| c.members.get(i))
                .map(|m| format!("`{}` back where it was", m.id))
                .unwrap_or_else(|| "the tile".to_owned()),
            None => "the tile".to_owned(),
        },
        ([one], []) => format!("`{one}` out"),
        ([], [one]) => format!("`{one}` back in"),
        // A step that both adds and removes is a replacement, and a multi-member step is a resize or
        // a fresh tile. Counted rather than listed: a line naming six pieces is not read.
        _ => format!("{} out, {} in", gone.len(), came.len()),
    }
}

/// **Record what changed, and step back and forth through it.**
///
/// One system owning both halves, because the alternative is a flag: a separate recorder would see
/// undo's *own* write as a new edit and push it, so undo would undo itself. Handling the keys here
/// means the recorder already knows which writes were its own.
///
/// It watches the tile rather than hooking each verb, so every mutation is covered by construction —
/// a drop, a hole, a nudge, a turn, a removal, and whatever is added next. `refit` runs before this,
/// so a resize is part of the same step as the edit that caused it rather than a second one to undo.
pub fn tile_history(
    keyboard: Res<ButtonInput<KeyCode>>,
    live: Res<crate::keys::Live>,
    mode: Res<crate::tiles::Mode>,
    mut build: ResMut<Build>,
    mut history: ResMut<TileHistory>,
    mut state: ResMut<crate::tiles::ImportState>,
) {
    if *mode != crate::tiles::Mode::Tiles {
        return;
    }
    // First run on this tab: adopt what is open without calling it an edit.
    if history.seen.is_none() {
        history.seen = Some(build.open.clone());
        history.opened = build.opened;
    }

    // **A new tile starts a new history.**
    //
    // Reported by the author, 2026-08-12: *"it all works except for the last undo. It just goes back
    // to a different mesh instead of blank."* It did, because opening a tile is a change like any
    // other to a system that watches the tile — so `N` pushed the tile you had just left onto the
    // stack, and undoing past the blank walked back into it.
    //
    // That is the hazard this very type's note already argues against for the two *tabs*: *"Two tabs
    // editing different files through one stack would make 'undo' mean whichever thing was touched
    // last."* Two tiles through one stack is the same sentence one level down — and worse here,
    // because `Cmd+S` saves under the open tile's id, so an undo that quietly swapped the document
    // could write one tile's members over another's name.
    if history.opened != build.opened {
        history.past.clear();
        history.future.clear();
        history.run = None;
        history.seen = Some(build.open.clone());
        history.opened = build.opened;
        return;
    }

    let back = crate::keys::just_pressed(&keyboard, *live, crate::keys::Action::UndoBuild);
    let forward = crate::keys::just_pressed(&keyboard, *live, crate::keys::Action::RedoBuild);
    if back || forward {
        let step = if back {
            history.past.pop()
        } else {
            history.future.pop()
        };
        match step {
            Some(to) => {
                let from = build.open.clone();
                // Kept for the readout below — the stacks take the value, so the description has to
                // be made from a copy rather than from what was just pushed.
                let said = step_says(&from, &to);
                if back {
                    history.future.push(from);
                } else {
                    history.past.push(from);
                }
                build.open = to.clone();
                // The focus can outlive the members it indexed.
                let n = build.open.as_ref().map_or(0, |c| c.members.len());
                build.focus = build.focus.min(n.saturating_sub(1));
                history.seen = Some(to);
                // Stepping through history ends the run: the next nudge is a new act, not a
                // continuation of one the author has already walked away from.
                history.run = None;
                let what = if back { "undo" } else { "redo" };
                state
                    .status
                    .note(format!("{what}: {said} — {n} in the tile"));
            }
            None => state.status.note(
                if back {
                    "nothing to undo"
                } else {
                    "nothing to redo"
                }
                .to_owned(),
            ),
        }
        return;
    }

    // Not a history key, so anything different is an edit worth remembering.
    if history.seen.as_ref() != Some(&build.open) {
        // **A run of adjustments to one member is one step.**
        //
        // Reported by the author, 2026-08-12: *"if I bring one mesh in, then another, when I hit
        // undo it doesn't remove the second mesh I added."* It did not, because every keystroke was
        // its own entry — drop, nudge, nudge is three, so one `Cmd+Z` stepped back a nudge and left
        // the mesh where it was. The arrows also **repeat** at `keys::REPEAT_SECS`, so holding one
        // for a second buries the drop under seven entries and undo reads as dead.
        //
        // Ousterhout, *A Philosophy of Software Design* §6.7, is explicit that this is the UI's
        // decision and not the history's: a `History` manages actions, and *"the policy for grouping
        // actions"* belongs to the layer that knows what a user thinks one act is. Here that is
        // moving a piece: an author drags a wall into place and calls it one thing, however many
        // taps it took.
        //
        // So a continuing run **replaces** rather than pushes — the entry already on top holds the
        // state from before the run began, which is exactly where undo should land. Anything else
        // ends the run, so nudge / drop / nudge never merges across the drop.
        let now = adjusted_member(&history.seen.clone().flatten(), &build.open);
        let continuing = now.is_some() && now == history.run && !history.past.is_empty();
        if !continuing {
            let was = history.seen.take().flatten();
            history.past.push(was);
            if history.past.len() > BUILD_HISTORY {
                history.past.remove(0);
            }
        }
        history.run = now;
        // A new edit is a new branch: what was undone past is no longer reachable.
        history.future.clear();
        history.seen = Some(build.open.clone());
    }
}

/// The system half of [`refit`], after the verbs.
///
/// A system of its own rather than a call at the end of `build_keys`, because that function returns
/// from each verb's branch — one verb, one act — so "after the verbs" is a scheduling fact rather
/// than a place in a function body. Gated on the change flag, so a keystroke that moved nothing
/// costs nothing.
///
/// # It used to be gated on the TAB as well, and that was the bug
///
/// The guard read `*mode != Mode::Tiles`, so a tile's envelope could only follow its contents while
/// that one panel was open. Reported from the keyboard: *"the sizing of the tile around the mesh
/// doesn't take place until you enter the mesh or the tile editing… we want this to happen whenever
/// a mesh gets loaded."*
///
/// The envelope is **read off the contents** ([`fit_envelope`]) rather than authored, so which panel
/// an author happens to be looking at cannot be part of the answer — that is a second source of
/// truth about the same fact, wearing a tab for a name. It matters most for exactly the case
/// reported: [`fit_envelope`] measures a member through `library.get(id)`, so a piece whose
/// descriptor is not in the library **yet** spans nothing and the tile fits to one cell. When the
/// measurement lands, `Project` changes — and with the tab gate in place that change was only
/// noticed if you were standing on Tiles at that moment. Now it is noticed wherever you are, which
/// is what makes the tile size itself when the mesh arrives rather than when you next nudge it.
///
/// Still gated on the change flags, so this is free on a frame where nothing moved, and `Build`
/// exists wherever this is registered — the same tuple carries `build_keys`, which takes it too.
pub fn refit_tile(
    mut build: ResMut<Build>,
    project: Res<crate::project::Project>,
) {
    if !(build.is_changed() || project.is_changed()) {
        return;
    }
    // **Refit, and say nothing.**
    //
    // This used to raise a *problem* whenever the envelope grew past one cell, and the author's own
    // log showed what that cost: fifteen of them from one continuous nudge — `2 x 3`, `2 x 4`,
    // `3 x 3`, `3 x 4`, `3 x 5`, `4 x 4` — none folding, because `Status` folds *consecutive
    // identical* lines and every one of these carried a different size. Problems are sticky by
    // design, so they outlived the tile: the panel read `MEMBERS: nothing yet` under twelve warnings
    // about a 4 x 4, plus a note that three more had been dropped at the cap.
    //
    // The fact was right and the shape was wrong. "This group is too big for the solver" is a
    // **property of the tile**, not an event in its history — `docs/ui.md` §3.2, show the state where
    // the state lives. It is one line in the TILE block now, true exactly while it is on screen, and
    // it costs the alert budget (§3.4) nothing.
    refit(&mut build, &project.library, project.lattice.cell_height);
}

/// **Every BUILD verb, in one system.**
///
/// One system rather than eight, because the verbs share the tile in hand and Bevy would otherwise
/// need eight `ResMut<Build>` and a schedule opinion about their order. `editor.rs`'s key dispatcher
/// is the same shape for the same reason.
///
/// Takes `Res<Project>` and only escalates to `ResMut` where it writes — `redraw_stamps` is gated on
/// `Project::is_changed()`, and Bevy flags a resource when a system *dereferences* `ResMut`, not when
/// it mutates. A verb that correctly decides to do nothing must not look like an edit.
#[allow(clippy::too_many_arguments)]
pub fn build_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    // The camera, so an arrow means what it points at on screen rather than a world axis.
    rig: Res<crate::view::Rig>,
    live: Res<crate::keys::Live>,
    mode: Res<crate::tiles::Mode>,
    mut build: ResMut<Build>,
    mut state: ResMut<crate::tiles::ImportState>,
    // The right-hand list's filter, so arriving on the tab arms a piece that is actually on screen.
    filters: Res<crate::filter::Filters>,
    suggestions: Res<crate::labels::Suggestions>,
    // **`ResMut`, dereferenced mutably only in the save branch.** Bevy flags a resource changed when
    // a system *dereferences* `ResMut`, not when it mutates — and `editor::redraw_stamps` is gated on
    // `Project::is_changed()`, so a keystroke that correctly does nothing would otherwise tear down
    // and rebuild every stamped row in the map. That defect is on the record (2026-08-11 inspection,
    // D3: eighteen rebuilds of an identical picture in one session).
    mut project: ResMut<crate::project::Project>,
) {
    use crate::keys::{Action, just_pressed};
    if *mode != crate::tiles::Mode::Tiles {
        return;
    }
    let pressed = |a: Action| just_pressed(&keyboard, *live, a);

    // **Arriving on the tab opens a tile**, so the first keystroke does something rather than asking
    // for another. This hung off a mode key until the tab existed; the tab becoming live is the same
    // moment, and one fewer thing to press.
    if build.open.is_none() {
        // **As tall as the space it fills**, taken from the map rather than from a constant. A number
        // here would be one facility's ceiling height baked into the editor — the mistake
        // `stack::datum` records fixing, where `OnCeiling` was hardcoded 2.4 m and hung the lights of
        // a 3.5 m room in mid-air. The map states its own height; that is the only number entitled to
        // answer this.
        open_blank(&mut build, &project);
        // **Arrive with something in hand.** Only fires when nothing was ever picked — the selection
        // otherwise persists — and without it the first `Enter` is a refusal, which is the worst
        // possible first impression of a tab. Liapis names the failure: a tool that will not let the
        // designer converge is where user fatigue starts.
        //
        // **From the list as it is filtered**, which is what every other selection path walks
        // (`tiles::library_ids`). Taking `descriptors.first()` armed a piece the filter was hiding:
        // no row highlighted, `keep_library_selection_visible` unable to correct it, and the first
        // Enter dropping something the author never saw and did not choose.
        if state.editing(&project.library).is_none()
            && let Some(first) =
                crate::tiles::library_ids(&project, &filters, true, Some(&suggestions))
                    .into_iter()
                    .next()
        {
            state.selected_library_id = Some(first);
        }
        let id = build
            .open
            .as_ref()
            .map(|c| c.id.clone())
            .unwrap_or_default();
        // Named keys only if they are live on this tab — `T F G H` stood here for two commits after
        // the tab split moved them to the Meshes lattice, which is a status line teaching dead keys.
        state.status.note(format!(
            "building `{id}` — up/down walk the library, Enter drops, Cmd+S saves"
        ));
        return;
    }

    let Some(size) = build.open.as_ref().and_then(|c| match c.envelope {
        Envelope::Bounded { size } => Some(size),
        Envelope::Anchored => None,
    }) else {
        return;
    };

    // **Take the piece, or put it back.** The door between the arrows walking the library list and
    // the arrows walking the tile — the author's *"a key to start the placement so the arrows don't
    // get elsewhere"*. `Esc` is the Global put-back verb and already means exactly this.
    if pressed(Action::BuildArm) {
        build.placing = !build.placing;
        state.status.note(if build.placing {
            "placing — arrows move the tile, Enter drops, Esc puts it back".to_owned()
        } else {
            "arrows walk the library".to_owned()
        });
        return;
    }
    // **`Esc` leaves the kit list**, which is invariant 2 of `docs/tiles_tab_contract.md` -- "Esc
    // always returns to Choosing" -- extended one stance further rather than given a second key.
    if build.browsing.is_some() && just_pressed(&keyboard, *live, Action::Cancel) {
        build.browsing = None;
        return;
    }
    if build.placing && just_pressed(&keyboard, *live, Action::Cancel) {
        build.placing = false;
        state.status.note("arrows walk the library".to_owned());
        return;
    }

    // **Shift+arrow puts the focused mesh flush against that side.** Before the nudge below, and
    // reached only when Shift is down — the two are stated as a `bs` pair so the plain arrow does
    // not swallow the chord.
    let mut align = Vec2::ZERO;
    if build.placing {
        if pressed(Action::AlignLeft) {
            align.x -= 1.0;
        }
        if pressed(Action::AlignRight) {
            align.x += 1.0;
        }
        if pressed(Action::AlignForward) {
            align.y -= 1.0;
        }
        if pressed(Action::AlignBack) {
            align.y += 1.0;
        }
    }
    if align != Vec2::ZERO {
        // **An empty tile is guidance, not a refusal**, so it stays a note the next keystroke
        // replaces rather than a line that sticks in the problem log until `Esc`. Asked before the
        // door rather than inside it, because everything the door refuses *is* a refusal.
        if !focused(&build) {
            state
                .status
                .note("nothing to flush — Enter brings the picked mesh in".to_owned());
            return;
        }
        let dir = step_in_view(align, rig.yaw);
        let focus = build.focus;
        // The span is read before the door, because "this is a hole" is the same kind of guidance:
        // a hole has no width, so "flush" is its position *on* the boundary — which `validate`
        // refuses. Nudging is how a hole is placed; there is nothing to align.
        let span = build
            .open
            .as_ref()
            .and_then(|c| c.members.get(focus))
            .and_then(|m| match &m.body {
                Body::Descriptor { id, .. } => project
                    .library
                    .get(id)
                    .map(|d| crate::editor::brush_span(d, m.yaw, MEMBERS_STAND_UP)),
                _ => None,
            });
        let Some(span) = span else {
            state
                .status
                .note("a hole has no width to put flush — nudge it instead".to_owned());
            return;
        };
        let was = build
            .open
            .as_ref()
            .and_then(|c| c.members.get(focus))
            .map(|m| m.at);
        let moved = edit(&mut build, &project.library, |comp| {
            let Some(m) = comp.members.get_mut(focus) else {
                return Err("the focused member went away".to_owned());
            };
            m.at = aligned(m.at, span, size, dir);
            Ok(m.at)
        });
        match moved {
            // **A flush that moves nothing says why.**
            //
            // Found by authoring: a 0.1 x 1.0 m wall flushed *along its length* is a genuine no-op —
            // `aligned` returns `(size/2 - span/2) * dir`, and a piece already spanning the tile on
            // that axis is already as flush as it can be. The arithmetic is right and the picture
            // does not move, which is indistinguishable from a key that never arrived. I did it twice
            // in a row with the source open, and only found out by reading the RON afterwards.
            //
            // This is the `refused`-versus-`did nothing` gap `docs/2026-08-11-editor-visual-inspection.md`
            // names as D2, in a new place: *"The information exists; only the channel is missing."*
            // A note rather than a problem, because nothing went wrong — it names the axis that would
            // move instead, which is the thing the author actually wants to know.
            Ok(to) if was == Some(to) => {
                let across = if dir.0 != 0 { "up/down" } else { "left/right" };
                state.status.note(format!(
                    "already flush that way — this piece spans the tile on that axis. Shift+{across} \
                     moves it across instead"
                ));
            }
            Ok(to) => state
                .status
                .note(format!("flush — ({:+.3}, {:+.3})", to.0, to.1)),
            Err(e) => state.status.problem(e),
        }
        return;
    }

    // **Step the focus through the members.**
    //
    // The verb `Build::focus` never had. It is what `R`, `Delete`, the arrows and the flush act on,
    // and it is drawn in amber — and until now a drop set it and nothing else could. Reported from
    // the keyboard, 2026-08-12: *"how do I switch between two meshes to edit its placement?"*
    //
    // Saturating rather than wrapping: the ends of a short list are somewhere an author can feel,
    // and a focus that jumps from the last member to the first is the largest possible move on the
    // smallest possible keystroke — the argument `SnapLevel::finer` already makes.
    let step_focus =
        i32::from(pressed(Action::MemberNext)) - i32::from(pressed(Action::MemberPrev));
    if step_focus != 0 {
        let n = build.open.as_ref().map_or(0, |c| c.members.len());
        if n == 0 {
            state
                .status
                .note("nothing in the tile yet — Enter brings the picked mesh in".to_owned());
            return;
        }
        let want = (build.focus as i32 + step_focus).clamp(0, n as i32 - 1) as usize;
        build.focus = want;
        let named = build
            .open
            .as_ref()
            .and_then(|c| c.members.get(want))
            .map(|m| m.id.clone())
            .unwrap_or_default();
        state
            .status
            .note(format!("`{named}` — {} of {n}", want + 1));
        return;
    }

    // **Empty the tile.** The shifted form of removing one member, on the `RemoveTile`/`DemoteTile`
    // precedent. One `edit` call, so the history records it as one step and `Cmd+Z` brings the whole
    // tile back — which is what makes it safe to offer at all.
    if pressed(Action::ClearTile) {
        let n = build.open.as_ref().map_or(0, |c| c.members.len());
        if n == 0 {
            state.status.note("the tile is already empty".to_owned());
            return;
        }
        match edit(&mut build, &project.library, |comp| {
            comp.members.clear();
            Ok(())
        }) {
            Ok(()) => {
                build.focus = 0;
                state.status.note(format!(
                    "emptied — {n} taken out, {} puts them back",
                    crate::keys::chord(Action::UndoBuild)
                ));
            }
            Err(e) => state.status.problem(e),
        }
        return;
    }

    // **The arrows move the member, not a cursor.**
    //
    // There is no cursor any more, and that is the point: a brought-in mesh lands centred and the
    // arrows adjust *it*, so the thing the author is looking at is the thing the keys act on. A
    // separate cursor was a second answer to "where are we", and under an envelope that fits its
    // contents the two disagreed — the cursor said a cell, the tile said a size, and dropping in the
    // middle cell grew a one-tile floor to 2 x 2. It survived one commit longer as a derived readout
    // and went stale at once; the panel reads the member now. See [`Build::focus`].
    //
    // **The `if build.placing` that used to wrap this is gone**, and its absence is the point: these
    // four bindings declare `Stance::Holding`, so `keys::just_pressed` refuses them with nothing in
    // hand. The guard was correct and invisible — a rule about when a key fires, kept somewhere the
    // key table could not state it. `keys::Stance` is where it lives now.
    // **All four arrows move the piece.** `step_in_view` maps a screen wish through the camera yaw
    // to one of the four world axes, so `up` is whatever up looks like from here -- north-east on
    // this isometric view -- and the sideways pair costs nothing but the wish.
    //
    // Two of them used to walk the member list, which the author reported as unintuitive and was:
    // the arrows offered half the directions the screen suggests and the other half did something
    // unrelated. The walk is `,` and `.` now.
    let mut wish = Vec2::ZERO;
    // Screen wishes are negative up, the convention `view::pan_direction` already reads.
    if pressed(Action::BuildForward) {
        wish.y -= 1.0;
    }
    if pressed(Action::BuildBack) {
        wish.y += 1.0;
    }
    if pressed(Action::BuildLeft) {
        wish.x -= 1.0;
    }
    if pressed(Action::BuildRight) {
        wish.x += 1.0;
    }
    let lift_by = i32::from(pressed(Action::BuildUp)) - i32::from(pressed(Action::BuildDown));
    if wish != Vec2::ZERO || lift_by != 0 {
        // Nothing in the tile is not an error, it is an empty tile — say what to press, as a note.
        if !focused(&build) {
            state
                .status
                .note("nothing to move yet — Enter brings the picked mesh in".to_owned());
            return;
        }
        let (dx, dz) = if wish == Vec2::ZERO {
            (0, 0)
        } else {
            step_in_view(wish, rig.yaw)
        };
        let focus = build.focus;
        // **The ladder is the piece's own**: its stops divide the span between the tile's centre
        // and the flush position [`aligned`] reaches, so both are exactly reachable at every depth
        // — the two verbs land on the same outermost stop by construction. A hole has no width, so
        // its ladder spans the whole tile; its boundary stop is the one place the door refuses, by
        // name, which is the honest answer for a slot that must sit strictly inside.
        let span = build
            .open
            .as_ref()
            .and_then(|c| c.members.get(focus))
            .map(|m| match &m.body {
                Body::Descriptor { id, .. } => project
                    .library
                    .get(id)
                    .map(|d| crate::editor::brush_span(d, m.yaw, MEMBERS_STAND_UP))
                    .unwrap_or((0.0, 0.0)),
                _ => (0.0, 0.0),
            })
            .unwrap_or((0.0, 0.0));
        let f = (
            flush_reach(size.0, span.0).max(0.0),
            flush_reach(size.2, span.1).max(0.0),
        );
        // **A piece that fills the tile on the pressed axis has no travel there** — guidance, not a
        // refusal, the same door-manners as the flush no-op. The floor is the usual case: a
        // one-cell piece in a one-cell tile has nowhere to go that would not grow the tile.
        let travel = (dx != 0 && f.0 > ON_STOP) || (dz != 0 && f.1 > ON_STOP);
        if !travel && lift_by == 0 {
            state.status.note(
                "this piece fills the tile on that axis — [ and ] move layers instead".to_owned(),
            );
            return;
        }
        let divisor = project.lattice.snap_divisor;
        let depth = build.depth;
        let was = build
            .open
            .as_ref()
            .and_then(|c| c.members.get(focus))
            .map(|m| (m.at, m.lift));
        let moved = edit(&mut build, &project.library, |comp| {
            let Some(m) = comp.members.get_mut(focus) else {
                return Err("the focused member went away".to_owned());
            };
            // **The next stop on the ladder, not a fixed pitch** — [`ladder_step`] carries the
            // rationale and the keyboard report that asked for it.
            m.at.0 = ladder_step(m.at.0, f.0, divisor, depth, dx);
            m.at.1 = ladder_step(m.at.1, f.1, divisor, depth, dz);
            // The floor is the floor: a member cannot be nudged under the tile it is in.
            m.lift = (m.lift + lift_by as f32 * lift_pitch(divisor, depth)).max(0.0);
            Ok((m.at, m.lift))
        });
        match moved {
            // The terminal stop: the ladder ends where the piece meets the tile, so a press outward
            // there moves nothing — said, because it is indistinguishable from a dead key otherwise.
            Ok(now) if was == Some(now) => state.status.note(
                "already at the flush stop — the ladder ends where the piece meets the tile"
                    .to_owned(),
            ),
            Ok(((x, z), lift)) => {
                state
                    .status
                    .note(format!("({:+.3}, {:+.3}) at {:.3} m", x, z, lift));
            }
            Err(e) => state.status.problem(e),
        }
        return;
    }

    // **The depth, latched and cycling.** Coarse is the span itself — centre and flush are the only
    // stops, so the first press from centre lands flush — then thirds of it, then ninths, then round
    // again. Asked for at the keyboard: *"press J once for smaller grid ... a third press would
    // reset to original."*
    if pressed(Action::BuildRung) {
        build.depth = (build.depth + 1) % DEPTHS;
        let n = project
            .lattice
            .snap_divisor
            .saturating_pow(build.depth)
            .max(1);
        state.status.note(match build.depth {
            0 => "grid: centre and flush — one press spans the tile".to_owned(),
            _ => format!("grid: centre to flush in {n} steps — J deepens, and wraps"),
        });
        return;
    }

    // **Drop what the library list has picked.** The piece is chosen on the mesh tab and dropped
    // here, which is the same right-hand list serving both tabs rather than a second browser — the
    // objection §3.2 of the compose-authoring plan raised against adding one.
    if pressed(Action::BuildDrop) {
        let Some(d) = state.editing(&project.library).cloned() else {
            state
                .status
                .problem("nothing picked — choose a piece in the list first".to_owned());
            return;
        };
        // **A member must name a library descriptor.** `ImportState::editing` falls back to the
        // focused *candidate* when nothing in the library is selected, and a candidate is a mesh
        // measured but not imported — so dropping one writes a tile naming an id `library.ron` does
        // not carry, which expands to nothing at stamp time. Refused at the door rather than
        // written and discovered later, which is this crate's rule for staged edits.
        if project.library.get(&d.id).is_none() {
            state.status.problem(format!(
                "`{}` is not in the library yet — accept it on the Meshes tab first",
                d.id
            ));
            return;
        }
        match edit(&mut build, &project.library, |comp| place(comp, &d)) {
            Ok(i) => {
                // **Focus follows the drop.** `insert_sorted` answers where it landed, and the two
                // verbs that act on "this member" — turn and remove — mean the one you just put
                // down. Ignoring the index left them acting on whatever sorted first, which is a
                // different piece as soon as a tile holds two.
                build.focus = i;
                // **A drop leaves you adjusting what you dropped**, whichever key brought it in.
                // `Enter` alone used to leave `placing` false and the arrows dead over a focused
                // member — the author's first report. `Esc` is the way back to the library list.
                build.placing = true;
                let n = build.open.as_ref().map_or(0, |c| c.members.len());
                state.status.note(format!(
                    "`{}` dropped — {n} in the tile. Arrows move it, Esc goes back to the list",
                    d.id
                ));
            }
            Err(e) => state.status.problem(e),
        }
        return;
    }

    // A hole instead of a piece. `accepts` comes from the project's slot axis, first token — the
    // vocabulary is the closed list, so there is nothing here to invent.
    if pressed(Action::BuildSlot) {
        let Some(accepts) = project.vocab.slot.names().next().map(str::to_owned) else {
            state.status.problem(
                "no `slot` tokens declared — add one to vocab.ron before dropping a hole"
                    .to_owned(),
            );
            return;
        };
        match edit(&mut build, &project.library, |comp| {
            place_slot(comp, &accepts)
        }) {
            Ok(i) => {
                build.focus = i;
                state.status.note(format!("hole for `{accepts}` dropped"));
            }
            Err(e) => state.status.problem(e),
        }
        return;
    }

    // Turn and remove act on the **last member dropped**, which is what `focus` tracks. Turning is a
    // quarter, because a tile is a quarter-turn object: `from_compositions` learns one prototype per
    // quarter and anything between is a tile the solver cannot reproduce.
    if pressed(Action::BuildTurn) {
        if !focused(&build) {
            state
                .status
                .note("nothing to turn yet — Enter brings the picked mesh in".to_owned());
            return;
        }
        // `focus` read before the mutable borrow — the closure would otherwise hold `build` twice.
        // Through the door like every other verb: a quarter turn changes `brush_span`, so it changes
        // what a piece touches and therefore what is mounted on it.
        let focus = build.focus;
        let turned = edit(&mut build, &project.library, |comp| {
            let Some(m) = comp.members.get_mut(focus) else {
                return Err("the focused member went away".to_owned());
            };
            m.yaw = (m.yaw + 90.0).rem_euclid(360.0);
            Ok(format!("`{}` turned to {:.0}", m.id, m.yaw))
        });
        match turned {
            Ok(said) => state.status.note(said),
            Err(e) => state.status.problem(e),
        }
        return;
    }
    if pressed(Action::BuildDropMember) {
        if !focused(&build) {
            state.status.note("nothing to remove".to_owned());
            return;
        }
        let focus = build.focus;
        let removed = edit(&mut build, &project.library, |comp| {
            if focus >= comp.members.len() {
                return Err("the focused member went away".to_owned());
            }
            Ok(comp.members.remove(focus).id)
        });
        match removed {
            Ok(id) => {
                // Clamped to what is left rather than stepped back: removing the first member should
                // leave the focus on the new first, not underflow to the last.
                let left = build.open.as_ref().map_or(0, |c| c.members.len());
                build.focus = build.focus.min(left.saturating_sub(1));
                state.status.note(format!("`{id}` removed"));
            }
            // **A host cannot be removed out from under what rests on it.** `rebind_hosts` refuses
            // inside the door, so the tile is untouched and the refusal names the fixture — where
            // before, the removal went through and left the sibling pointing at a member that no
            // longer existed. `validate_shape` then refused the whole composition, `expand` blanked
            // the stage, and no verb in the tab could put it back.
            Err(e) => state.status.problem(e),
        }
        return;
    }

    // **`Cmd+S` saves the tile *and* the map.** The other half of the branch `editor::keys` guards:
    // the key is Global because the verb is, and what it saves is whatever the live context has open.
    // Bound once, so the census still holds every action to exactly one key.
    //
    // Both files, because an author reaches this tab from the Map with unsaved work behind them and
    // `editor::keys` — the only call to `Project::save` in the crate — steps aside for this branch.
    // Saving only the composition answered *"`kit/tile_1` saved — 3 members"*, which reads as a
    // successful save, while twenty Map edits stayed in memory and left with the process.
    //
    // **Independently, and both reported.** They are two files and neither one's refusal is a reason
    // to withhold the other: a tile that will not validate must not also cost the map its save.
    if pressed(Action::Save) {
        // **A tile the author never named asks before it is written.** `open_blank` still mints a
        // provisional id so the tab is usable the moment it opens — an editor that demanded a name
        // before it would show you anything would be worse — but a provisional name must not reach
        // the kit, because the KIT list is where it would be read back.
        if build.provisional && build.open.is_some() {
            build.naming = Some(NamePrompt {
                raw: String::new(),
                then: NameThen::Save,
            });
            state.status.note(
                "name this tile before it is saved — Enter saves it, Esc goes back".to_owned(),
            );
            return;
        }
        // **Saving a tile saves the tile.** It used to save the open map in the same breath, back
        // when every door had one behind it — a convenience for an author who had just stamped the
        // thing they were editing. The Tiles door has no map, so the second half of that pair has
        // nothing to write, and a verb whose name is "save the tile" doing two writes was already
        // more than it said. The Maps door still saves the map, on its own key.
        match save(&build, &mut project) {
            Ok(said) => state.status.note(said),
            Err(e) => state.status.problem(format!("TILE NOT SAVED: {e}")),
        }
        return;
    }

    // **A fresh tile, and it is named before it exists.**
    //
    // `N` used to mint `<kit>/tile_N` and open it, so every tile an author made was named by the
    // editor and there was no verb to say otherwise. That is fine while tiles are invisible and
    // wrong the moment the KIT list shows them back: a list of `tile_1 … tile_9` is a list nobody
    // can navigate. Asked for at the keyboard, 2026-08-15 — naming should be explicit.
    //
    // The prompt is `chrome::NameBox`, the same centred field the Map names a composition in, and
    // `naming_keys` below owns the keystrokes. The tile is opened by `Enter`, not by this press.
    if pressed(Action::BuildNew) {
        build.naming = Some(NamePrompt {
            raw: String::new(),
            then: NameThen::Open,
        });
        state
            .status
            .note("name the tile — Enter opens it, Esc leaves things as they are".to_owned());
        return;
    }

    // ── the kit ──────────────────────────────────────────────────────────────────────────────
    //
    // Walking a list and opening from it. The tab could make tiles and never show them, so an
    // author had no way to see the kit, reopen a tile to correct it, or notice they had built the
    // same thing twice.
    let kit = project.compositions.compositions.len();
    if pressed(Action::KitEnter) {
        if kit == 0 {
            // A note, not a problem: an empty kit is where every project starts, and the answer is
            // to make one rather than to fix anything.
            state
                .status
                .note("no tiles in the kit yet — build one and press Cmd+S".to_owned());
        } else {
            build.browsing = Some(0);
        }
    }
    if let Some(row) = build.browsing {
        let step = |row: usize, by: i32| -> usize {
            // Saturating, like the member walk: an author holding an arrow at the end of a list
            // should stop there rather than wrap to the other end of it.
            (row as i32 + by).clamp(0, kit.saturating_sub(1) as i32) as usize
        };
        if pressed(Action::KitPrev) {
            build.browsing = Some(step(row, -1));
        }
        if pressed(Action::KitNext) {
            build.browsing = Some(step(row, 1));
        }
        // **`left` goes back to the mesh list**, the ascend half of the column browser. `Esc` still
        // does it too and always did — that is the tab's global "not that" — but an author reaching
        // for `left` after `right` brought them in is following the idiom, not looking for a second
        // escape hatch.
        if pressed(Action::KitLeave) {
            build.browsing = None;
            state.status.note("back to the meshes".to_owned());
            return;
        }
        if pressed(Action::KitOpen) {
            match project.compositions.compositions.get(row).cloned() {
                Some(comp) => {
                    let id = comp.id.clone();
                    let n = comp.members.len();
                    open_saved(&mut build, comp);
                    state.status.note(format!(
                        "`{id}` opened — {n} member(s), Cmd+S saves over it"
                    ));
                }
                // The list is drawn from the same slice this indexes, so this is unreachable rather
                // than unlikely — said out loud because the alternative is an `unwrap`.
                None => state
                    .status
                    .problem(format!("no tile at row {row}; the kit has {kit}")),
            }
        }
    }
}

/// **The keystrokes of the tile name prompt.**
///
/// `Phase::Text`, and it drains the stream when the field is shut — the `xseam` guard every field in
/// this crate carries, so the `N` that opens the prompt cannot become its first character.
///
/// `Enter` opens the named tile (or saves the open one, when the prompt was raised by `Cmd+S`);
/// `Esc` leaves everything as it was. The name is forced to snake_case as it is typed, the way the
/// Map's composition prompt teaches the same rule.
pub fn naming_keys(
    mut events: bevy::prelude::MessageReader<bevy::input::keyboard::KeyboardInput>,
    mode: Res<crate::tiles::Mode>,
    mut build: ResMut<Build>,
    mut project: ResMut<crate::project::Project>,
    mut state: ResMut<crate::tiles::ImportState>,
) {
    if build.naming.is_none() || *mode != crate::tiles::Mode::Tiles {
        events.clear();
        return;
    }
    for event in events.read() {
        if !event.state.is_pressed() {
            continue;
        }
        match &event.logical_key {
            bevy::input::keyboard::Key::Enter => {
                let Some(prompt) = build.naming.clone() else {
                    return;
                };
                let name = emerge_core::naming::to_snake_case(&prompt.raw);
                if name.is_empty() {
                    state
                        .status
                        .problem("a tile needs a name — type one, or Esc to leave it".to_owned());
                    return;
                }
                let id = format!("{}/{}", project.namespace, name);
                if project.compositions.compositions.iter().any(|c| c.id == id) {
                    // Refused by name rather than silently replacing somebody's tile.
                    state
                        .status
                        .problem(format!("`{id}` is already in the kit — pick another name"));
                    return;
                }
                build.naming = None;
                // **What the author asked for, not what the state looks like** — see [`NameThen`].
                if prompt.then == NameThen::Save {
                    if let Some(open) = build.open.as_mut() {
                        open.id = id.clone();
                    }
                    build.provisional = false;
                    match save(&build, &mut project) {
                        Ok(said) => state.status.note(said),
                        Err(e) => state.status.problem(format!("TILE NOT SAVED: {e}")),
                    }
                } else {
                    open_named(&mut build, &project, &id);
                    state.status.note(format!("building `{id}`"));
                }
                return;
            }
            bevy::input::keyboard::Key::Escape => {
                build.naming = None;
                state.status.note("left as it was".to_owned());
                return;
            }
            bevy::input::keyboard::Key::Backspace => {
                if let Some(prompt) = build.naming.as_mut() {
                    prompt.raw.pop();
                }
            }
            bevy::input::keyboard::Key::Character(c) => {
                if let Some(prompt) = build.naming.as_mut() {
                    // No whitespace: an id has none, and accepting a space would be accepting a
                    // keystroke that `to_snake_case` then throws away.
                    if c.chars().all(|c| !c.is_whitespace()) {
                        prompt.raw.push_str(c);
                    }
                }
            }
            _ => {}
        }
    }
}

/// **Open a blank tile at the default rung.**
///
/// Both places that open one go through here, so "what state does a new tile start in" has one
/// answer rather than two that drift.
fn open_blank(build: &mut Build, project: &crate::project::Project) {
    let comp = blank(&next_tile_id(project), project.lattice.cell_height);
    // The editor chose this name, so the save door will ask before it reaches the kit.
    build.provisional = true;
    build.depth = DEFAULT_DEPTH;
    build.open = Some(comp);
    build.focus = 0;
    // The one place a different tile becomes the open one, so the one place the boundary is marked.
    build.opened = build.opened.wrapping_add(1);
}

/// **Open a blank tile under the name the author just typed.**
///
/// `open_blank`'s twin, and it goes through the same fields for the same reason: "what state does a
/// new tile start in" has one answer rather than two that drift.
fn open_named(build: &mut Build, project: &crate::project::Project, id: &str) {
    let comp = blank(id, project.lattice.cell_height);
    build.provisional = false;
    build.depth = DEFAULT_DEPTH;
    build.open = Some(comp);
    build.focus = 0;
    build.placing = false;
    build.opened = build.opened.wrapping_add(1);
}

/// **Reopen an authored tile for editing.**
///
/// The verb the tab never had. `open_blank` was the only opener, so every tile was a new one and a
/// tile saved wrong stayed wrong — an author who put a piece in the middle of a tile instead of
/// flush against its edge could only fix it by editing `compositions.ron` by hand.
///
/// Goes through the same fields `open_blank` sets, including `opened`, because this is a different
/// document by exactly the argument that field carries: an undo that crossed the boundary would
/// write one tile's members under another's name.
pub fn open_saved(build: &mut Build, comp: Composition) {
    // Off disk, therefore named — whatever it is called.
    build.provisional = false;
    build.depth = DEFAULT_DEPTH;
    build.open = Some(comp);
    build.focus = 0;
    // **Reopening a tile IS holding it**, and the first version of this said the opposite: "the
    // arrows should walk, not move, until something is picked up". There is nothing to pick up in a
    // reopened tile -- the members are already in it -- so that landed the author in `Idle` with a
    // tile on screen they could not touch, and `,`/`.` bound at `Holding` did nothing at all. They
    // reported it within a minute of the verb existing.
    //
    // The third time this tab has keyed the stance on how you ARRIVED rather than on what there is
    // to do; `docs/tiles_tab_contract.md` carries the other two. `focused` still decides, so a tile
    // reopened with no members is `Idle`, which is right -- there is genuinely nothing to move.
    build.placing = true;
    build.browsing = None;
    build.opened = build.opened.wrapping_add(1);
}

/// The next unused `<namespace>/tile_n` id, so a new tile opens rather than asking for a name first.
///
/// A composition id shares a descriptor id's shape — namespace and all — and a tile that does not
/// carry its kit's namespace is one nobody can find later. **The namespace is `Project::namespace`,
/// resolved once at open**, which is where the rule and its refusals live; this used to derive it
/// here *and* in `kit_namespace`, both reading `descriptors.first()` and both substituting the
/// literal `"kit"` — two copies of one answer, and an answer that depended on sort order.
fn next_tile_id(project: &crate::project::Project) -> String {
    let kit = &project.namespace;
    for n in 1..=project.compositions.compositions.len() + 1 {
        let id = format!("{kit}/tile_{n}");
        if !project.compositions.compositions.iter().any(|c| c.id == id) {
            return id;
        }
    }
    format!("{kit}/tile")
}

/// One member of the tile as it currently stands on the stage.
#[derive(Component)]
pub struct StagedTile;

/// **Stand the tile up** — every member, through the same expander and the same spawner the Map uses.
///
/// # It goes through `composition::expand`, not straight at the members
///
/// `expand` is *"the one expander. The editor's ghost, the editor's save and the game's loader all
/// come through here, so a stamp cannot look one way in the tool and another in the game."* Spawning
/// the members directly would be a second reading of the same data — it would skip host resolution,
/// nested groups and paint order, and the tile would look right here and wrong once stamped. That is
/// the exact failure this crate keeps being rewritten to avoid.
///
/// # Rebuilt whole, on change
///
/// The tile is a handful of members and rebuilding is a despawn plus a spawn; incremental patching
/// would be a second model of what is on the stage, to be kept in step with the first. Gated on the
/// resources actually read, so a keystroke that changes nothing redraws nothing — the D3 defect
/// (2026-08-11) is what a rebuild fired on `is_changed()` alone costs.
pub fn drive_build_preview(
    mut commands: Commands,
    assets: Res<AssetServer>,
    mode: Res<crate::tiles::Mode>,
    build: Res<Build>,
    project: Res<crate::project::Project>,
    mut state: ResMut<crate::tiles::ImportState>,
    staged: Query<Entity, With<StagedTile>>,
) {
    // **`ImportState` is in here because the ghost reads the selection**, which lives there — without
    // it, picking a different piece left the previous one standing under the cursor.
    if !(build.is_changed() || mode.is_changed() || project.is_changed() || state.is_changed()) {
        return;
    }
    // A system that stops running cannot despawn what it drew, so leaving the mode clears here rather
    // than being gated away.
    for e in &staged {
        commands.entity(e).despawn();
    }
    if *mode != crate::tiles::Mode::Tiles {
        return;
    }
    let Some(comp) = build.open.as_ref() else {
        return;
    };
    let Envelope::Bounded { size } = comp.envelope else {
        return;
    };

    // **The piece in hand, standing where it would land.**
    //
    // This tab had no ghost at all, which is what the author hit on first use: *"Selected a 'pallet'
    // mesh: issue 1, no mesh appeared."* Nothing was drawn until `Enter`, so picking a piece and
    // aiming it were both invisible and the tab looked broken before it looked slow. The Map has had
    // a ghost since the beginning and this module's own opening line is *"the ghost is the
    // contract"* — it was the one surface that did not keep it.
    //
    // Drawn at [`BROUGHT_IN`] — **the value `place` writes**, not arithmetic that agrees with it.
    // The two were separate: the ghost stood on cell-corner arithmetic left over from the deleted
    // cursor while the drop landed centred, so a 0.1 m wall previewed flush against the west edge
    // and arrived 450 mm away in a tile a metre across. `y` is the tile floor: the ghost is a
    // where-not-a-what, and asking `resolve_y` about a piece that is not in the tile yet would need
    // a scratch map holding a member that does not exist.
    //
    // **And it previews only what `Enter` will accept.** `ImportState::editing` falls back to the
    // focused *candidate*, which the drop branch refuses by name — so ghosting it stood a piece in
    // the tile that the very next keystroke would not put there. A preview is a promise.
    //
    // **While choosing, not only while placing.** This was gated on `build.placing`, which showed
    // the ghost after a piece was taken and never while one was being picked — exactly backwards:
    // asked for at the keyboard, 2026-08-14: *"when I select a mesh, but haven't yet hit enter ...
    // there should be a semitransparent rendering of the mesh selected. Like a preview."* The one
    // stance it stays out of is Browsing — the kit list selects a tile, not a mesh, and a mesh
    // ghost under a tile cursor would be previewing the wrong kind of thing.
    if build.browsing.is_none()
        && let Some(d) = state
            .editing(&project.library)
            .filter(|d| project.library.get(&d.id).is_some())
            .cloned()
    {
        let (at, lift) = BROUGHT_IN;
        let stage = crate::stages::TILE;
        // **The mount's height and the mesh's `y_offset`, on top of the brought-in lift.**
        //
        // `BROUGHT_IN` is the XZ and the layer `place` writes, and that half is still read straight
        // from it — the ghost must stand where the drop lands. What it did not carry is the piece's
        // own datum: `stack::resolve_y` adds `mount` height and `align.y_offset` to every *committed*
        // member a few lines below, so a wall light ghosted on the tile floor and then jumped up its
        // 1.4 m the instant `Enter` landed it. Measured over BRP 2026-08-18 as a 0.31 m disagreement
        // with the Meshes stage on the same piece.
        //
        // Through `tiles::staged_lift`, which is where the Meshes stage reads it too, so the two
        // cannot drift apart again.
        let lift = lift + crate::tiles::staged_lift(&d);
        if let Some(e) = crate::editor::spawn_piece(
            &mut commands,
            &assets,
            &d,
            at,
            0.0,
            (0, 0),
            (stage.x, stage.y, stage.z),
            lift,
        ) {
            // **`Ghost` as well as `StagedTile`.** `editor::fade_ghost` is the one thing that makes
            // a preview translucent and it queries that marker alone, so without it the "ghost"
            // drew solid and shadow-casting — indistinguishable from a committed member, which
            // makes `Enter` look like it did nothing and `Esc` look like it deleted something.
            // `StagedTile` is what despawns it on the next rebuild.
            commands
                .entity(e)
                .insert((StagedTile, crate::editor::Ghost));
        }
    }

    if comp.members.is_empty() {
        return;
    }

    // **The envelope as a map**, floor at zero and the declared bounds — the same scratch
    // `composition::interface` builds, so `stack::resolve_y` answers here exactly as it will in the
    // game. The stage's own offset is applied by `spawn_piece`'s `origin`, so the tile is authored at
    // the origin and drawn four kilometres away.
    let scratch = emerge_core::map::Map {
        version: emerge_core::map::MAP_VERSION,
        name: "build_stage".to_owned(),
        origin: (0.0, 0.0, 0.0),
        bounds: size,
        ..Default::default()
    };
    // The tile in hand is not on disk yet, so it is handed to `expand` alongside the saved ones —
    // which is also what lets a member nest a composition that *is* saved.
    let mut comps = project.compositions.compositions.clone();
    match comps.iter().position(|c| c.id == comp.id) {
        Some(i) => comps[i] = comp.clone(),
        None => comps.push(comp.clone()),
    }
    let stamp = emerge_core::composition::Stamped {
        id: "build".to_owned(),
        of: comp.id.clone(),
        ..Default::default()
    };
    let expanded =
        match emerge_core::composition::expand(&scratch, &[stamp], &comps, &project.library) {
            Ok(e) => e,
            // **Named, not swallowed.** A tile that cannot stand up is the author's next problem, and
            // a silently empty stage looks exactly like a tile with nothing in it.
            Err(e) => {
                if state.status.line() != e {
                    state.status.problem(e);
                }
                return;
            }
        };
    let mut with_rows = scratch.clone();
    with_rows
        .placements
        .extend(expanded.placements.iter().cloned());
    let ys = match emerge_core::stack::resolve_y(&with_rows, &project.library) {
        Ok(ys) => ys,
        Err(e) => {
            if state.status.line() != e {
                state.status.problem(e);
            }
            return;
        }
    };

    let stage = crate::stages::TILE;
    for (k, p) in expanded.placements.iter().enumerate() {
        let Some(base) = project.library.get(&p.descriptor) else {
            continue;
        };
        let d = match &p.patch {
            Some(patch) => base.patched_with(patch),
            None => base.clone(),
        };
        let Some(&y) = ys.get(k) else { continue };
        // **The row this member expanded into** — `expand` mints rows `"{stamp}/{member_path}"`
        // and the stamp here is `"build"`, so the focused member `wall_low` is the row
        // `build/wall_low` and every leaf of a nested group under it shares the prefix.
        let held = build.placing
            && comp.members.get(build.focus).is_some_and(|m| {
                p.id.strip_prefix("build/")
                    .is_some_and(|path| path == m.id || path.starts_with(&format!("{}/", m.id)))
            });
        if let Some(e) = crate::editor::spawn_piece(
            &mut commands,
            &assets,
            &d,
            p.at,
            p.yaw,
            p.tip,
            (stage.x, stage.y, stage.z),
            y,
        ) {
            commands.entity(e).insert(StagedTile);
            if held {
                commands.entity(e).insert(crate::editor::HeldPiece);
            }
        }
    }
}

/// **The tile's box, its grid, and the cell the cursor is on.**
///
/// Gizmos rather than meshes: they are drawn per frame and need no despawning, which is right for
/// something that moves every keystroke. The cursor is the thing an author steers by — without it,
/// walking the grid moves a number in a panel and nothing on the stage.
pub fn draw_build_grid(
    mode: Res<crate::tiles::Mode>,
    build: Res<Build>,
    project: Res<crate::project::Project>,
    mut gizmos: Gizmos,
) {
    if *mode != crate::tiles::Mode::Tiles {
        return;
    }
    let Some(Envelope::Bounded { size }) = build.open.as_ref().map(|c| c.envelope) else {
        return;
    };
    let stage = crate::stages::TILE;
    let centre = stage + Vec3::new(0.0, size.1 * 0.5, 0.0);

    // The envelope. `cube` takes a transform whose SCALE is the size — 0.19 spells it that way.
    gizmos.cube(
        Transform::from_translation(centre).with_scale(Vec3::new(size.0, size.1, size.2)),
        crate::chrome::DIM,
    );

    // A third of a cell — the old unit rung, kept as the orientation grid and as the box drawn for
    // a thing with no mesh of its own.
    let unit = emerge_core::grid::TILE / project.lattice.snap_divisor.max(1) as f32;

    // **The focused member and the stops its arrows walk.** The ladder is per axis and per piece —
    // a wall's travel is wider across than along — so the drawn lines ARE the reachable positions,
    // which is the whole complaint the span ladder answers: a grid the piece lands beside is a grid
    // that lies.
    let focus = build.focus;
    let member = build.open.as_ref().and_then(|c| c.members.get(focus));
    if let Some(m) = member {
        let span = match &m.body {
            Body::Descriptor { id, .. } => project
                .library
                .get(id)
                .map(|d| crate::editor::brush_span(d, m.yaw, MEMBERS_STAND_UP))
                // A hole has no width, so its ladder spans the whole tile.
                .unwrap_or((0.0, 0.0)),
            _ => (0.0, 0.0),
        };
        if build.placing {
            let n = project
                .lattice
                .snap_divisor
                .saturating_pow(build.depth)
                .max(1) as i32;
            let f = (
                flush_reach(size.0, span.0).max(0.0),
                flush_reach(size.2, span.1).max(0.0),
            );
            for k in -n..=n {
                // Skip an axis the piece fills: 2n+1 lines drawn on top of each other read as one
                // line claiming travel that does not exist.
                if f.0 > ON_STOP {
                    let x = if k == n {
                        f.0
                    } else if k == -n {
                        -f.0
                    } else {
                        k as f32 * f.0 / n as f32
                    };
                    gizmos.line(
                        stage + Vec3::new(x, 0.0, -size.2 * 0.5),
                        stage + Vec3::new(x, 0.0, size.2 * 0.5),
                        crate::chrome::GRID_LINE,
                    );
                }
                if f.1 > ON_STOP {
                    let z = if k == n {
                        f.1
                    } else if k == -n {
                        -f.1
                    } else {
                        k as f32 * f.1 / n as f32
                    };
                    gizmos.line(
                        stage + Vec3::new(-size.0 * 0.5, 0.0, z),
                        stage + Vec3::new(size.0 * 0.5, 0.0, z),
                        crate::chrome::GRID_LINE,
                    );
                }
            }
        }
        let span = if span == (0.0, 0.0) {
            (unit, unit)
        } else {
            span
        };
        let height = match &m.body {
            Body::Descriptor { id, .. } => project
                .library
                .get(id)
                .and_then(|d| d.extent.height)
                .unwrap_or(unit),
            _ => unit,
        };
        // **The focused member**, boxed in the accent colour — the one thing on the stage that
        // answers "what do the arrows move". There is no cursor: the member *is* the selection.
        gizmos.cube(
            Transform::from_translation(stage + Vec3::new(m.at.0, m.lift + height * 0.5, m.at.1))
                .with_scale(Vec3::new(span.0, height, span.1)),
            crate::chrome::ACCENT,
        );
    }
    // Nothing in hand: the unit lattice for orientation, bounded by a cell count so it stops at
    // the tile — the squares tiles abut on, not stops any arrow claims to walk.
    if member.is_none() || !build.placing {
        let (nx, _, nz) = cells(size, unit);
        gizmos.grid(
            Isometry3d::new(stage, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
            UVec2::new(nx, nz),
            Vec2::splat(unit),
            crate::chrome::GRID_LINE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TILE: (f32, f32, f32) = (1.0, 2.4, 1.0);
    /// The default divisor's first rung — thirds of a tile.
    const FINE: f32 = 1.0 / 3.0;

    #[test]
    fn a_tile_divides_into_the_rungs_number_of_cells() {
        assert_eq!(
            cells(TILE, FINE),
            (3, 7, 3),
            "1 m across is 3 thirds; 2.4 m up is 7"
        );
        assert_eq!(
            cells(TILE, 1.0),
            (1, 2, 1),
            "the bare rung is the whole tile"
        );
        // Never zero, whatever the pitch: a grid with no cells has nowhere to stand.
        assert_eq!(cells(TILE, 99.0), (1, 1, 1));
    }

    /// A tile in hand, ready for the verbs below.
    fn open(id: &str) -> Build {
        Build {
            open: Some(blank(id, 2.4)),
            depth: DEFAULT_DEPTH,
            ..Default::default()
        }
    }

    /// The drop, through the same door `build_keys` presses `Enter` into.
    fn drop_in(
        b: &mut Build,
        lib: &emerge_core::library::Library,
        d: &emerge_core::descriptor::Descriptor,
    ) -> Result<usize, String> {
        edit(b, lib, |comp| place(comp, d))
    }

    fn desc(id: &str) -> Member {
        Member {
            id: id.to_owned(),
            body: Body::Descriptor {
                id: format!("site/{id}"),
                tip: (0, 0),
                on: None,
                patch: None,
            },
            at: (0.0, 0.0),
            yaw: 0.0,
            lift: 0.0,
            paint: 0,
            of_fingerprint: None,
            note: None,
        }
    }

    /// Ids come from the piece and are made unique by a suffix — nobody types a member name.
    #[test]
    fn member_ids_are_derived_and_never_collide() {
        let mut ms: Vec<Member> = Vec::new();
        assert_eq!(fresh_id(&ms, "site/wall"), "wall");
        ms.push(desc("wall"));
        assert_eq!(fresh_id(&ms, "site/wall"), "wall_2");
        ms.push(desc("wall_2"));
        assert_eq!(fresh_id(&ms, "site/wall"), "wall_3");
        // A piece with no namespace keeps its whole name.
        assert_eq!(fresh_id(&ms, "floor"), "floor");
    }

    /// **A vocabulary token is not an id**, and a hole's member id is derived from one.
    ///
    /// `vocab.rs` documents tokens like `"uses-electricity"` — hyphens are correct there and illegal
    /// in an id. Seeding a member id with the token directly produced a member
    /// `composition::validate` refuses, so **a tile carrying any hole could not be saved**, and both
    /// slot tokens the real project declares are hyphenated.
    #[test]
    fn a_slot_id_is_a_legal_id_whatever_the_token_looks_like() {
        let ms: Vec<Member> = Vec::new();
        assert_eq!(slot_id(&ms, "wall-fixture").as_deref(), Ok("wall_fixture"));
        assert_eq!(slot_id(&ms, "floor-decal").as_deref(), Ok("floor_decal"));
        // Whatever it produces must satisfy the rule it exists to satisfy, checked rather than
        // assumed — the two can otherwise drift apart silently.
        for token in [
            "wall-fixture",
            "floor-decal",
            "a b c",
            "Trailing--",
            "UPPER",
        ] {
            if let Ok(id) = slot_id(&ms, token) {
                assert!(
                    emerge_core::naming::is_id(&id),
                    "`{token}` produced `{id}`, not an id"
                );
            }
        }
        // And a token that cannot yield one is refused by name, not renamed into something that
        // happens to parse.
        assert!(
            slot_id(&ms, "2nd-socket").is_err(),
            "an id cannot start with a digit"
        );
        assert!(slot_id(&ms, "---").is_err(), "nothing to make an id out of");
        // Uniquing still applies, since it is the same job `fresh_id` does for pieces.
        let taken = vec![desc("wall_fixture")];
        assert_eq!(
            slot_id(&taken, "wall-fixture").as_deref(),
            Ok("wall_fixture_2")
        );
    }

    /// **A hole stays strictly inside its tile, wherever the arrows put it.**
    ///
    /// `composition::validate` refuses a slot *on* the seam — *"a slot exactly on the seam is the
    /// ambiguous case"* — and the envelope is read off the members, so the two have to agree at the
    /// boundary or a hole can nudge itself into a tile that cannot be saved. It could: `fit_envelope`
    /// gave a slot no footprint, so the envelope came out at exactly `2·|at|` and the hole landed on
    /// the seam it had just defined. At the shipped divisor of 3 that took three presses of one
    /// arrow; at 2 it took one.
    ///
    /// Walked out to several tiles in every direction rather than tested at one offset, because the
    /// failure is periodic — it recurs at every cell boundary, not only the first.
    #[test]
    fn a_nudged_hole_never_lands_on_the_seam_it_creates() {
        let lib = kit();
        for divisor in [2u32, 3, 4, 9] {
            let step = 1.0 / divisor as f32;
            for nudges in 0..=(divisor as i32 * 3) {
                let mut b = open("kit/t");
                edit(&mut b, &lib, |comp| place_slot(comp, "wall-fixture"))
                    .unwrap_or_else(|e| panic!("the hole drops: {e}"));
                for _ in 0..nudges {
                    edit(&mut b, &lib, |comp| {
                        let m = comp.members.get_mut(0).ok_or("no member")?;
                        m.at.0 += step;
                        Ok(())
                    })
                    .unwrap_or_else(|e| panic!("a nudge moves the hole: {e}"));
                    refit(&mut b, &lib, 2.4);
                }
                let open = b.open.take().unwrap_or_else(|| panic!("the tile is open"));
                open.validate_shape().unwrap_or_else(|e| {
                    panic!("divisor {divisor}, {nudges} nudges: the tile must still save: {e}")
                });
            }
        }
    }

    /// **The list stays in the canonical order**, so what an author reads is what gets written and
    /// `composition::validate` never has to refuse the file the editor just produced.
    #[test]
    fn members_are_kept_sorted_by_id() {
        let mut ms = Vec::new();
        for id in ["wall", "floor", "sconce"] {
            insert_sorted(&mut ms, desc(id));
        }
        let ids: Vec<&str> = ms.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["floor", "sconce", "wall"]);
    }

    /// The whole gesture, end to end: open a tile, walk the grid, drop a floor and a wall, and get a
    /// group `composition::validate` accepts.
    #[test]
    fn a_built_tile_validates() {
        let mut b = open("site/tile_wall_n");
        let lib = kit();
        let floor = lib
            .get("site/floor")
            .unwrap_or_else(|| panic!("the kit has a floor"));
        let wall = lib
            .get("site/wall")
            .unwrap_or_else(|| panic!("the kit has a wall"));

        drop_in(&mut b, &lib, floor).expect("the floor drops");
        drop_in(&mut b, &lib, wall).expect("the wall drops");

        let comp = b.open.take().expect("still open");
        assert_eq!(comp.members.len(), 2);
        emerge_core::composition::validate(&[comp], &lib).expect("a built tile is a legal tile")
    }

    /// A hole drops with the same gesture and lands inside the envelope — which is what
    /// `composition::validate` requires of one.
    #[test]
    fn a_dropped_slot_lands_inside_the_envelope() {
        let lib = kit();
        let mut b = open("t");
        edit(&mut b, &lib, |comp| place_slot(comp, "wall-fixture")).expect("the hole drops");

        let comp = b.open.take().expect("still open");
        let m = comp
            .members
            .first()
            .unwrap_or_else(|| panic!("the hole is a member"));
        assert!(
            m.at.0.abs() < 0.5 && m.at.1.abs() < 0.5,
            "{:?} is on or past a seam",
            m.at
        );
        assert!(
            m.lift >= 0.0 && m.lift < 2.4,
            "lift {} leaves the envelope",
            m.lift
        );
    }

    /// Dropping into a tile that was never opened is a refusal that says what to press, not a panic
    /// and not a silently-discarded keystroke.
    #[test]
    fn dropping_with_no_tile_open_says_so() {
        let mut b = Build::default();
        let lib = kit();
        let floor = lib
            .get("site/floor")
            .unwrap_or_else(|| panic!("the kit has a floor"));
        let e = drop_in(&mut b, &lib, floor).expect_err("nothing to drop into");
        assert!(e.contains("no tile open"), "{e}");
    }

    /// **A fixture is mounted on the member it was dropped against** — the author's own sentence,
    /// *"a wall mounted light fixture on the wall mesh"*, which was the one clause of it that could
    /// not be authored. `on` was hardcoded `None`, meaning "find a host outside this group", and a
    /// face-mounted piece with no host makes `stack::resolve_y` refuse — so the **map would not
    /// load**. Automatic rather than a verb, by the author's call: a fixture dropped against a wall
    /// has already said which wall it means.
    #[test]
    fn a_fixture_is_bound_to_the_wall_it_is_dropped_against() {
        let lib = kit();
        let wall = lib
            .get("site/wall")
            .unwrap_or_else(|| panic!("the kit has a wall"));
        let sconce = lib
            .get("site/sconce")
            .unwrap_or_else(|| panic!("the kit has a sconce"));

        let mut b = open("kit/t");
        drop_in(&mut b, &lib, wall).expect("the wall drops");
        // **`lift` is zero, so the height under test is the mount's alone.** `Member::lift` is *"a
        // vertical nudge on top of whatever the mount resolves to"* — additive by schema — so
        // nudging it up first would resolve to 1.8 plus that and prove nothing about the face.
        drop_in(&mut b, &lib, sconce).expect("the sconce drops onto the wall");

        let comp = b.open.take().expect("still open");
        let m = comp
            .members
            .iter()
            .find(|m| m.id == "sconce")
            .unwrap_or_else(|| {
                panic!(
                    "the sconce is a member: {:?}",
                    comp.members.iter().map(|m| &m.id).collect::<Vec<_>>()
                )
            });
        let Body::Descriptor { on, .. } = &m.body else {
            panic!("a dropped piece is a descriptor member");
        };
        assert_eq!(
            on.as_deref(),
            Some("wall"),
            "the fixture must name the wall it is on, or the map refuses to load"
        );

        // **And the tile stands up**, which is the claim that actually matters: `resolve_y` is what
        // refused before, `emerge-bevy` propagates it with `?`, so this failing is a map that will
        // not load. Same two calls the 3-D preview makes, so what passes here is what the author
        // sees on the stage.
        let scratch = emerge_core::map::Map {
            version: emerge_core::map::MAP_VERSION,
            name: "t".to_owned(),
            bounds: (1.0, 2.4, 1.0),
            ..Default::default()
        };
        let stamp = emerge_core::composition::Stamped {
            id: "s".to_owned(),
            of: comp.id.clone(),
            ..Default::default()
        };
        let expanded = emerge_core::composition::expand(&scratch, &[stamp], &[comp], &lib)
            .unwrap_or_else(|e| panic!("the tile must expand: {e}"));
        let mut with_rows = scratch.clone();
        with_rows
            .placements
            .extend(expanded.placements.iter().cloned());
        let ys = emerge_core::stack::resolve_y(&with_rows, &lib).unwrap_or_else(|e| {
            panic!("a bound fixture must resolve, or the map will not load: {e}")
        });

        // The sconce rides its wall's face at the height the mount declares, rather than sitting on
        // the floor — which is the difference between "it loaded" and "it is where it belongs".
        let k = expanded
            .placements
            .iter()
            .position(|p| p.descriptor == "site/sconce")
            .unwrap_or_else(|| panic!("the sconce is a row"));
        let y = ys.get(k).copied().unwrap_or_default();
        assert!(
            (y - 1.8).abs() < 1e-4,
            "the sconce should ride the face at 1.8 m, got {y}"
        );
    }

    /// **Four arrows, four different squares — at every yaw the camera can sit at.**
    ///
    /// This is the test whose absence let two of the four arrows do nothing. The dominant-component
    /// rule collapsed at exactly the iso yaw, where screen-up and screen-left have equal `x` and `z`
    /// magnitude, so both resolved to `-x`. Nothing caught it: the unit test asserted "exactly one
    /// axis moves", which was still true of a rule that sent half the keys to the same axis, and the
    /// camera had been turned square-on in the same breath, which hid it on screen.
    ///
    /// So the property is **distinctness**, not single-axis-ness — and it is checked at the detents
    /// *and* between them, because the failure lived exactly on a boundary and a test at the
    /// cardinal yaws alone would have sailed past it.
    #[test]
    fn the_four_arrows_are_four_different_axes_at_every_yaw() {
        use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI};
        let wishes = [
            ("up", Vec2::new(0.0, -1.0)),
            ("down", Vec2::new(0.0, 1.0)),
            ("left", Vec2::new(-1.0, 0.0)),
            ("right", Vec2::new(1.0, 0.0)),
        ];
        // Detents, half-detents, and a couple of arbitrary angles — the bug was ON a boundary, so
        // "some angle in the middle of a quadrant" is the case that would have missed it.
        let yaws = [
            0.0,
            FRAC_PI_4,
            FRAC_PI_2,
            3.0 * FRAC_PI_4,
            PI,
            -FRAC_PI_2,
            -FRAC_PI_4,
            0.3,
            1.9,
            -2.7,
        ];
        for yaw in yaws {
            let steps: Vec<(i32, i32)> =
                wishes.iter().map(|(_, w)| step_in_view(*w, yaw)).collect();
            for (i, s) in steps.iter().enumerate() {
                assert!(
                    (s.0 == 0) ^ (s.1 == 0),
                    "yaw {yaw}: `{}` must move exactly one axis, got {s:?}",
                    wishes[i].0
                );
                for (j, other) in steps.iter().enumerate().skip(i + 1) {
                    assert_ne!(
                        s, other,
                        "yaw {yaw}: `{}` and `{}` must not be the same square — {steps:?}",
                        wishes[i].0, wishes[j].0
                    );
                }
            }
        }
    }

    /// **At the framing the author actually uses, the arrows read the way they are drawn.**
    ///
    /// The mapping is derived rather than typed, so this is the one place it is stated as a fact
    /// anyone can check against the screen: at the default iso yaw, up is north.
    #[test]
    fn at_the_iso_yaw_up_is_minus_z() {
        assert_eq!(step_in_view(Vec2::new(0.0, -1.0), 0.0), (0, -1), "up is -z");
        assert_eq!(step_in_view(Vec2::new(0.0, 1.0), 0.0), (0, 1), "down is +z");
        assert_eq!(
            step_in_view(Vec2::new(-1.0, 0.0), 0.0),
            (-1, 0),
            "left is -x"
        );
        assert_eq!(
            step_in_view(Vec2::new(1.0, 0.0), 0.0),
            (1, 0),
            "right is +x"
        );
    }

    /// **Flush is a function of the piece's own width, which is why it is a verb.**
    ///
    /// `site/wall` is 0.1 m thick and sits flush at -0.45 in a 1 m tile. No rung of any divisor
    /// lands on -0.45 — 1/3 gives ±0.333, 1/9 gives ±0.444 — so it is not somewhere the arrows can
    /// step to, however fine the ladder. That is the same fact `policy.rs` recorded about seating:
    /// art is authored to look right rather than to tile.
    #[test]
    fn a_wall_reaches_the_tile_edge_that_no_rung_lands_on() {
        let tile = (1.0, 2.4, 1.0);
        // 0.1 m thick, standing across the cell.
        let wall = (0.1, 1.0);
        assert_eq!(
            aligned((0.0, 0.0), wall, tile, (-1, 0)),
            (-0.45, 0.0),
            "flush left"
        );
        assert_eq!(
            aligned((0.0, 0.0), wall, tile, (1, 0)),
            (0.45, 0.0),
            "flush right"
        );

        // **And no rung reaches it**, which is the reason this verb exists rather than a finer step.
        for divisor in [2u32, 3, 4, 6, 8] {
            let rung = 1.0 / divisor as f32;
            let steps = (0.45f32 / rung).round();
            assert!(
                (steps * rung - 0.45).abs() > 1e-4,
                "a divisor of {divisor} would make the align verb redundant"
            );
        }

        // Only the pressed axis moves — flushing left must not also recentre front to back.
        let off = (0.2, 0.31);
        let out = aligned(off, wall, tile, (-1, 0));
        assert_eq!(out.1, 0.31, "the other axis is left alone");

        // A wide piece flushes by its own half-width, in a tile that has grown to hold it.
        let two = (1.0, 2.4, 2.0);
        let back = aligned((0.0, 0.0), (0.81, 1.21), two, (0, -1)).1;
        assert!((back + 0.395).abs() < 1e-5, "flush back, got {back}");
    }

    /// **The tile is as many whole cells as its contents need — and no more.**
    ///
    /// The author's model, in their words: *"as many whole tiles as needed to capture the object…
    /// if the mesh is adjusted and tiles are no longer needed, they're automatically removed… if it
    /// falls on the seam, more tiles are added."* Both directions are checked here, because a rule
    /// that only grows is the easy half and the shrink is what makes it feel like it is tracking
    /// rather than accumulating.
    #[test]
    fn the_tile_grows_and_shrinks_to_hold_what_is_in_it() {
        use emerge_core::descriptor::{Descriptor, Extent};
        let piece = |id: &str, w: f32, d: f32| Descriptor {
            id: id.to_owned(),
            extent: Extent {
                footprint: Some((w, d)),
                height: Some(1.0),
            },
            ..Default::default()
        };
        let lib = emerge_core::library::Library {
            version: emerge_core::library::LIBRARY_VERSION,
            note: None,
            descriptors: vec![piece("small", 0.4, 0.4), piece("pallet", 0.81, 1.21)],
        };
        let tile = emerge_core::grid::TILE;

        // Nothing in it is one cell, not zero.
        assert_eq!(fit_envelope(&[], &lib, 2.4), (tile, 2.4, tile));

        // A piece that fits stays one cell.
        let at_origin = |id: &str| Member {
            id: id.to_owned(),
            body: Body::Descriptor {
                id: id.to_owned(),
                tip: (0, 0),
                on: None,
                patch: None,
            },
            at: (0.0, 0.0),
            yaw: 0.0,
            lift: 0.0,
            paint: 0,
            of_fingerprint: None,
            note: None,
        };
        assert_eq!(
            fit_envelope(&[at_origin("small")], &lib, 2.4),
            (tile, 2.4, tile)
        );

        // **1.21 m needs two cells, not one and a bit.** It reaches 0.605 from the anchor and one
        // cell only reaches 0.5 — the envelope is centred, so this is the arithmetic that decides it.
        let got = fit_envelope(&[at_origin("pallet")], &lib, 2.4);
        assert_eq!(
            (got.0, got.2),
            (tile, 2.0 * tile),
            "0.81 fits one cell, 1.21 does not"
        );

        // **Moved onto a seam, it takes more.** Shifted half a cell, the pallet reaches 1.105 and
        // needs three.
        let mut shifted = at_origin("pallet");
        shifted.at = (0.0, 0.5);
        let got = fit_envelope(&[shifted], &lib, 2.4);
        assert_eq!(got.2, 3.0 * tile, "on the seam it takes another cell");

        // **And moved back, it gives them up again** — the half the author asked for by name.
        let got = fit_envelope(&[at_origin("pallet")], &lib, 2.4);
        assert_eq!(got.2, 2.0 * tile, "tiles no longer needed are removed");
    }

    /// **Resizing says when the group stops being solver content.**
    ///
    /// `grammar::from_compositions` takes tiles of the grid's size and skips anything else by name.
    /// That is not an error — a bigger group is still stamped by hand, and it is what Compose
    /// composes — but an author who thinks they are building a tile should learn it here rather than
    /// from a generate that quietly never uses what they made.
    #[test]
    fn growing_past_one_cell_says_the_solver_will_not_place_it() {
        use emerge_core::descriptor::{Descriptor, Extent};
        let pallet = Descriptor {
            id: "pallet".to_owned(),
            extent: Extent {
                footprint: Some((0.81, 1.21)),
                height: Some(0.2),
            },
            ..Default::default()
        };
        let lib = emerge_core::library::Library {
            version: emerge_core::library::LIBRARY_VERSION,
            note: None,
            descriptors: vec![pallet.clone()],
        };

        let mut b = open("kit/t");
        assert!(
            refit(&mut b, &lib, 2.4).is_none(),
            "an empty tile is one cell and says nothing"
        );

        drop_in(&mut b, &lib, &pallet).expect("it drops");
        let grew = refit(&mut b, &lib, 2.4).expect("growing past one cell is a change");
        // **The size, not a sentence about it.** This used to return the warning text and
        // `refit_tile` raised it as a sticky problem on every size change — fifteen deep from one
        // nudge, still on screen after the tile was emptied. The fact is a property of the tile now
        // and the panel states it beside the size; see `is_one_cell`.
        assert_eq!(
            tiles_across(grew),
            (1, 2),
            "it grew to two tiles on Z: {grew:?}"
        );
        assert!(
            !is_one_cell(grew),
            "and that is the thing the panel qualifies the size with"
        );

        // Silent when nothing moved, or `Build`'s change flag would rebuild the stage every frame.
        assert!(
            refit(&mut b, &lib, 2.4).is_none(),
            "a refit that changes nothing writes nothing"
        );
    }

    /// **A tile is as tall as the storey it is built for, and follows that number when it changes.**
    ///
    /// `blank` takes the height from a number rather than a constant for the reason `stack::datum`
    /// records: a hardcoded 2.4 m ceiling hung the lights of a 3.5 m room in mid-air. `refit` then
    /// wrote all three axes but compared only two, so an author who changed the height and came back
    /// kept the old one for ever — and every `OnCeiling` fixture in that tile was wrong by the
    /// difference, in `compositions.ron` and in the game.
    ///
    /// **The number moved on 2026-08-16**, from `map.bounds.1` to `kits.ron`'s
    /// `Lattice::cell_height`, and that is a deliberate behaviour change rather than a port. A tile
    /// is a *kit* artifact: taking its height from whichever map was open meant the same kit made
    /// differently-shaped blank tiles depending on where you came from, and meant nothing at all on
    /// a door with no map. The cost is real and is the trade being made — a project whose maps have
    /// different ceiling heights now gets one tile height for all of them, and a kit built for two
    /// storey heights needs two kits.
    #[test]
    fn the_envelope_follows_the_storey_height() {
        let lib = kit();
        let mut b = open("kit/t");
        let floor = lib
            .get("site/floor")
            .unwrap_or_else(|| panic!("the kit has a floor"));
        drop_in(&mut b, &lib, floor).expect("the floor drops");

        // **A height change is a refit**, and `refit` now answers "did the envelope move" rather
        // than "is this worth warning about" — the warning was a sticky problem and is a live panel
        // line instead (see `refit_tile`). What must stay true is that following the map's height
        // does not make the tile un-generatable: it is still one cell across.
        let grew = refit(&mut b, &lib, 3.5).expect("following the map's height is a change");
        assert!(
            is_one_cell(grew),
            "a taller room must not cost the tile its solver eligibility: {grew:?}"
        );
        let Some(Envelope::Bounded { size }) = b.open.as_ref().map(|c| c.envelope) else {
            panic!("a tile claims a tile");
        };
        assert!(
            (size.1 - 3.5).abs() < 1e-4,
            "the envelope must follow the map, got {}",
            size.1
        );

        // And still silent when nothing at all moved, which is what the guard is there for.
        assert!(
            refit(&mut b, &lib, 3.5).is_none(),
            "a refit that changes nothing writes nothing"
        );
    }

    /// **Nothing to mount to is a refusal at the door**, not a member written now and a map that
    /// will not load later.
    #[test]
    fn a_fixture_with_no_wall_under_it_is_refused_when_it_is_dropped() {
        let lib = kit();
        let sconce = lib
            .get("site/sconce")
            .unwrap_or_else(|| panic!("the kit has a sconce"));
        let mut b = open("kit/t");
        let e = drop_in(&mut b, &lib, sconce).expect_err("nothing offers a face");
        assert!(
            e.contains("wall-inner"),
            "it must name the class it wanted: {e}"
        );
        assert!(
            b.open.as_ref().is_some_and(|c| c.members.is_empty()),
            "a refused drop leaves the tile exactly as it was"
        );
    }

    /// **Ambiguity refuses naming both**, rather than picking whichever sorts first — a silent
    /// choice a later sort could change is the shape this repo's determinism rules exist to forbid.
    #[test]
    fn a_fixture_touching_two_walls_is_refused_naming_them() {
        let lib = kit();
        let wall = lib
            .get("site/wall")
            .unwrap_or_else(|| panic!("the kit has a wall"));
        let sconce = lib
            .get("site/sconce")
            .unwrap_or_else(|| panic!("the kit has a sconce"));

        let mut b = open("kit/t");
        // Two walls in the same place, so the sconce cannot say which it means by where it is.
        drop_in(&mut b, &lib, wall).expect("the first wall drops");
        drop_in(&mut b, &lib, wall).expect("the second wall drops");
        let e = drop_in(&mut b, &lib, sconce).expect_err("which wall?");
        assert!(
            e.contains("wall") && e.contains("wall_2"),
            "it must name both: {e}"
        );
    }

    /// **A host cannot be taken out from under what rests on it.**
    ///
    /// `place` refuses the mirror image — a fixture with nothing to mount to — precisely so a tile
    /// carrying a dangling `on` cannot exist. Delete let one in by the back door: the wall went, the
    /// sconce went on naming it, and `validate_shape` then refused the whole composition
    /// ("rests on `wall`, which is not a member of it") with no verb in the tab able to repair it.
    /// The author's only route out was deleting the sconce as well.
    #[test]
    fn removing_a_wall_that_a_fixture_rests_on_is_refused() {
        let lib = kit();
        let wall = lib
            .get("site/wall")
            .unwrap_or_else(|| panic!("the kit has a wall"));
        let sconce = lib
            .get("site/sconce")
            .unwrap_or_else(|| panic!("the kit has a sconce"));

        let mut b = open("kit/t");
        drop_in(&mut b, &lib, wall).expect("the wall drops");
        drop_in(&mut b, &lib, sconce).expect("the sconce drops onto it");

        // Members are sorted by id, so the wall is index 1 — read it rather than assume it.
        let at = b
            .open
            .as_ref()
            .and_then(|c| c.members.iter().position(|m| m.id == "wall"))
            .unwrap_or_else(|| panic!("the wall is a member"));
        let e = edit(&mut b, &lib, |comp| Ok(comp.members.remove(at).id))
            .expect_err("the sconce is on it");
        assert!(
            e.contains("wall-inner"),
            "it must say what the sconce needed: {e}"
        );
        let comp = b.open.take().unwrap_or_else(|| panic!("the tile is open"));
        assert_eq!(
            comp.members.len(),
            2,
            "a refused removal leaves the tile exactly as it was"
        );
        emerge_core::composition::validate(&[comp], &lib)
            .expect("and the tile it leaves behind still saves");
    }

    /// **A move re-reads what everything is mounted on.**
    ///
    /// `on` was written once, at drop time, and no verb ever looked at it again — so flushing a wall
    /// against an edge left the sconce claiming to be on a wall 400 mm away. `composition::validate`
    /// only checks that the named sibling *exists*, so that tile **saved**, and `stack::resolve_y`
    /// then put the fixture at the face height with nothing under it: floating in mid-air, in the
    /// editor and in the game alike. `BuildTurn` had the same hole — a quarter turn changes
    /// `brush_span`, so it changes what a piece touches.
    #[test]
    fn moving_a_wall_out_from_under_a_fixture_is_refused_rather_than_silently_written() {
        let lib = kit();
        let wall = lib
            .get("site/wall")
            .unwrap_or_else(|| panic!("the kit has a wall"));
        let sconce = lib
            .get("site/sconce")
            .unwrap_or_else(|| panic!("the kit has a sconce"));

        let mut b = open("kit/t");
        drop_in(&mut b, &lib, wall).expect("the wall drops");
        drop_in(&mut b, &lib, sconce).expect("the sconce drops onto it");

        let at = b
            .open
            .as_ref()
            .and_then(|c| c.members.iter().position(|m| m.id == "wall"))
            .unwrap_or_else(|| panic!("the wall is a member"));
        // Flush left: the wall goes to -0.45 and the sconce, still at the centre, is 400 mm away.
        let e = edit(&mut b, &lib, |comp| {
            let m = comp.members.get_mut(at).ok_or("no wall")?;
            m.at = aligned(m.at, (0.1, 1.0), TILE, (-1, 0));
            Ok(())
        })
        .expect_err("that leaves the sconce on nothing");
        assert!(
            e.contains("wall-inner"),
            "it must name what the sconce lost: {e}"
        );

        let comp = b.open.take().unwrap_or_else(|| panic!("the tile is open"));
        let m = comp
            .members
            .iter()
            .find(|m| m.id == "sconce")
            .unwrap_or_else(|| panic!("the sconce is a member"));
        assert_eq!(
            m.at,
            (0.0, 0.0),
            "a refused move leaves every member where it was"
        );
        let Body::Descriptor { on, .. } = &m.body else {
            panic!("a dropped piece is a descriptor member");
        };
        assert_eq!(
            on.as_deref(),
            Some("wall"),
            "and still bound to the wall it is on"
        );
    }

    /// **A fixture follows its wall when the pair moves together**, which is the other half of the
    /// same rule — re-resolving must not mean refusing every move.
    #[test]
    fn a_fixture_and_its_wall_move_together() {
        let lib = kit();
        let wall = lib
            .get("site/wall")
            .unwrap_or_else(|| panic!("the kit has a wall"));
        let sconce = lib
            .get("site/sconce")
            .unwrap_or_else(|| panic!("the kit has a sconce"));

        let mut b = open("kit/t");
        drop_in(&mut b, &lib, wall).expect("the wall drops");
        drop_in(&mut b, &lib, sconce).expect("the sconce drops onto it");

        edit(&mut b, &lib, |comp| {
            for m in comp.members.iter_mut() {
                m.at.0 += FINE;
            }
            Ok(())
        })
        .expect("moving both keeps them together");

        let comp = b.open.take().unwrap_or_else(|| panic!("the tile is open"));
        let m = comp
            .members
            .iter()
            .find(|m| m.id == "sconce")
            .unwrap_or_else(|| panic!("the sconce is a member"));
        let Body::Descriptor { on, .. } = &m.body else {
            panic!("a dropped piece is a descriptor member");
        };
        assert_eq!(
            on.as_deref(),
            Some("wall"),
            "the binding survives a move of the pair"
        );
        emerge_core::composition::validate(&[comp], &lib).expect("and the tile still saves");
    }

    /// **Nothing hosts itself.** A wall offers the face it is asked about, so the moment `on` became
    /// something re-resolved rather than written once, a member could be handed itself as a host —
    /// which `composition::validate` refuses by name ("rests on itself").
    #[test]
    fn a_piece_is_never_offered_itself_as_a_host() {
        use emerge_core::descriptor::{Descriptor, Extent, Mount, Offers};
        // One piece that both offers `wall-inner` and mounts to it — the degenerate case.
        let mut post = Descriptor {
            id: "site/post".to_owned(),
            extent: Extent {
                footprint: Some((0.2, 0.2)),
                height: Some(2.4),
            },
            ..Default::default()
        };
        post.offers = Offers {
            faces: vec!["wall-inner".to_owned()],
            ..Default::default()
        };
        post.mount = Some(Mount::OnFace {
            class: "wall-inner".to_owned(),
            height: 1.0,
        });
        let lib = emerge_core::library::Library {
            version: emerge_core::library::LIBRARY_VERSION,
            note: None,
            descriptors: vec![post.clone()],
        };

        let mut b = open("kit/t");
        // Alone it has nothing to rest on and is refused, rather than resting on itself.
        let e = drop_in(&mut b, &lib, &post).expect_err("there is nothing else here");
        assert!(e.contains("wall-inner"), "{e}");
    }

    /// A four-piece kit with a real face relationship: the wall offers `wall-inner`, the sconce
    /// needs it. Built here rather than read from `assets/` — this crate's rule is that tests do not
    /// bind to the shipped corpus.
    fn kit() -> emerge_core::library::Library {
        use emerge_core::descriptor::{Descriptor, Extent, Mount, Offers};
        let piece = |id: &str, w: f32, d: f32, h: f32| Descriptor {
            id: id.to_owned(),
            extent: Extent {
                footprint: Some((w, d)),
                height: Some(h),
            },
            ..Default::default()
        };
        let mut wall = piece("site/wall", 0.1, 1.0, 2.4);
        wall.offers = Offers {
            faces: vec!["wall-inner".to_owned()],
            ..Default::default()
        };
        let mut sconce = piece("site/sconce", 0.2, 0.2, 0.3);
        sconce.mount = Some(Mount::OnFace {
            class: "wall-inner".to_owned(),
            height: 1.8,
        });
        emerge_core::library::Library {
            version: emerge_core::library::LIBRARY_VERSION,
            note: None,
            descriptors: vec![piece("site/floor", 1.0, 1.0, 0.06), wall, sconce],
        }
    }
}
